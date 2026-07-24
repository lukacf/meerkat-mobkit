//! Orchestrator: restore flow, reconciliation, and topology wiring.
//!
//! Implements REQ-12 (restore flow), REQ-13 (broken continuity), REQ-19/REQ-21
//! (topology reconciliation), REQ-20 (static wiring preserved), REQ-22 (topology
//! not continuity truth), and REQ-33 (identity-keyed reconciliation).

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use futures::{StreamExt, stream};

use super::contracts::{AgentCustomizer, TopologyProvider};
use super::runtime::{IdentityRuntime, IdentityRuntimeError};
use super::types::{
    AgentBuildContext, AgentBuildDraft, AgentIdentity, AgentRuntimeId, CheckpointVersion,
    ContinuityFailure, ContinuityFailureKind, ContinuityGeneration, ContinuityRecord,
    ContinuityResolveState, DurableAgentSpec, IdentityLifecycleState, LeaseAcquireResult,
    LeaseGrant, ManagedPeerEdge, SessionSnapshot, TopologyContext,
};

pub(crate) const IDENTITY_RESTORE_CONCURRENCY: usize = 4;

/// Effective restore concurrency: `MOBKIT_IDENTITY_RESTORE_CONCURRENCY`
/// overrides the default, clamped to [1, 16]. `1` serializes restores — the
/// field mitigation for Bug I, where concurrent multi-hundred-MB session
/// resumes against one SQLite store exceed the 5s writer busy_timeout and the
/// resulting mid-spawn store failures feed meerkat's destructive resume
/// rollback (upstream ask 31).
pub(crate) fn identity_restore_concurrency() -> usize {
    parse_identity_restore_concurrency(
        std::env::var("MOBKIT_IDENTITY_RESTORE_CONCURRENCY")
            .ok()
            .as_deref(),
    )
}

fn parse_identity_restore_concurrency(raw: Option<&str>) -> usize {
    raw.and_then(|value| value.trim().parse::<usize>().ok())
        .map(|value| value.clamp(1, 16))
        .unwrap_or(IDENTITY_RESTORE_CONCURRENCY)
}

#[cfg(test)]
mod restore_concurrency_tests {
    use super::{
        IDENTITY_RESTORE_CONCURRENCY, RestoreSnapshotPolicy, parse_identity_restore_concurrency,
        should_load_resume_snapshot,
    };

    #[test]
    fn defaults_when_unset_or_invalid() {
        assert_eq!(
            parse_identity_restore_concurrency(None),
            IDENTITY_RESTORE_CONCURRENCY
        );
        assert_eq!(
            parse_identity_restore_concurrency(Some("")),
            IDENTITY_RESTORE_CONCURRENCY
        );
        assert_eq!(
            parse_identity_restore_concurrency(Some("not-a-number")),
            IDENTITY_RESTORE_CONCURRENCY
        );
    }

    #[test]
    fn parses_and_clamps() {
        assert_eq!(parse_identity_restore_concurrency(Some("1")), 1);
        assert_eq!(parse_identity_restore_concurrency(Some(" 8 ")), 8);
        assert_eq!(parse_identity_restore_concurrency(Some("0")), 1);
        assert_eq!(parse_identity_restore_concurrency(Some("64")), 16);
    }

    #[test]
    fn public_restore_always_preserves_snapshot_payload() {
        assert!(should_load_resume_snapshot(
            RestoreSnapshotPolicy::PreserveOutcomePayload,
            Some(false),
            false,
        ));
    }

    #[test]
    fn bootstrap_skips_only_for_opted_out_bridge_during_live_resume() {
        assert!(!should_load_resume_snapshot(
            RestoreSnapshotPolicy::BridgeRequirementOnly,
            Some(false),
            false,
        ));
        assert!(should_load_resume_snapshot(
            RestoreSnapshotPolicy::BridgeRequirementOnly,
            Some(true),
            false,
        ));
        assert!(should_load_resume_snapshot(
            RestoreSnapshotPolicy::BridgeRequirementOnly,
            None,
            false,
        ));
    }

    #[test]
    fn already_active_reconcile_never_loads_a_snapshot() {
        assert!(!should_load_resume_snapshot(
            RestoreSnapshotPolicy::PreserveOutcomePayload,
            None,
            true,
        ));
        assert!(!should_load_resume_snapshot(
            RestoreSnapshotPolicy::BridgeRequirementOnly,
            Some(false),
            true,
        ));
        assert!(!should_load_resume_snapshot(
            RestoreSnapshotPolicy::BridgeRequirementOnly,
            Some(true),
            true,
        ));
    }
}

fn trace_identity_restore_completed(identity: &AgentIdentity, started_at: Instant) {
    let elapsed = started_at.elapsed();
    if elapsed >= Duration::from_secs(1) {
        tracing::info!(
            %identity,
            elapsed_ms = elapsed.as_millis(),
            "identity restore completed"
        );
    } else {
        tracing::debug!(
            %identity,
            elapsed_ms = elapsed.as_millis(),
            "identity restore completed"
        );
    }
}

// ---------------------------------------------------------------------------
// Restore flow result
// ---------------------------------------------------------------------------

fn durable_spec_uses_external_binding(spec: &DurableAgentSpec) -> bool {
    matches!(spec.backend, Some(meerkat_mob::MobBackendKind::External))
        || matches!(
            spec.binding.as_ref(),
            Some(meerkat_contracts::WireRuntimeBinding::External { .. })
        )
}

/// Result of the restore flow for a single identity.
#[derive(Debug, Clone)]
pub enum RestoreOutcome {
    /// Lazy bootstrap registered identity metadata without materializing a
    /// concrete mob member/session.
    Dormant {
        record: Option<ContinuityRecord>,
        draft: AgentBuildDraft,
    },
    /// Fresh-created: new AgentRuntimeId, new SessionId.
    Created {
        record: ContinuityRecord,
        draft: AgentBuildDraft,
    },
    /// Resumed from authoritative snapshot.
    Resumed {
        record: ContinuityRecord,
        snapshot: SessionSnapshot,
        draft: AgentBuildDraft,
    },
    /// Broken continuity — failed loudly per REQ-13.
    Broken(ContinuityFailure),
}

/// Result of the full restore flow.
#[derive(Debug)]
pub struct RestoreFlowResult {
    pub outcomes: BTreeMap<AgentIdentity, RestoreOutcome>,
    pub managed_edges: Vec<ManagedPeerEdge>,
}

// ---------------------------------------------------------------------------
// Reconciliation
// ---------------------------------------------------------------------------

/// Actions produced by reconciliation (REQ-33).
#[derive(Debug, Clone)]
pub enum ReconcileAction {
    /// New identity: resolve + lease + activate.
    Activate(DurableAgentSpec),
    /// Removed identity: retire.
    Retire(AgentIdentity),
    /// Changed spec: hot-reload metadata fields.
    HotReload {
        identity: AgentIdentity,
        new_spec: DurableAgentSpec,
    },
    /// Profile changed: requires respawn.
    Respawn {
        identity: AgentIdentity,
        new_spec: DurableAgentSpec,
    },
}

/// Compute reconciliation actions by comparing desired roster to current active set.
pub fn compute_reconcile_actions(
    desired: &[DurableAgentSpec],
    current: &BTreeMap<AgentIdentity, DurableAgentSpec>,
) -> Vec<ReconcileAction> {
    let mut actions = Vec::new();

    let desired_map: BTreeMap<&AgentIdentity, &DurableAgentSpec> =
        desired.iter().map(|s| (&s.identity, s)).collect();

    // New identities: in desired, not in current
    for (id, spec) in &desired_map {
        if !current.contains_key(*id) {
            actions.push(ReconcileAction::Activate((*spec).clone()));
        }
    }

    // Removed identities: in current, not in desired
    for id in current.keys() {
        if !desired_map.contains_key(id) {
            actions.push(ReconcileAction::Retire(id.clone()));
        }
    }

    // Changed identities: in both, spec differs
    for (id, new_spec) in &desired_map {
        if let Some(old_spec) = current.get(*id)
            && old_spec != *new_spec
        {
            // Profile change → respawn (REQ-33)
            if old_spec.profile == new_spec.profile {
                // labels, display_name, addressability → hot-reload
                // context, additional_instructions → registry update
                actions.push(ReconcileAction::HotReload {
                    identity: (*id).clone(),
                    new_spec: (*new_spec).clone(),
                });
            } else {
                actions.push(ReconcileAction::Respawn {
                    identity: (*id).clone(),
                    new_spec: (*new_spec).clone(),
                });
            }
        }
    }

    actions
}

async fn delete_tentative_continuity_record(
    runtime: &IdentityRuntime,
    identity: &AgentIdentity,
    grant: Option<&LeaseGrant>,
    persisted: bool,
) -> Option<String> {
    if !persisted {
        return None;
    }
    let grant = grant?;
    runtime
        .continuity_store()
        .delete_continuity_record(identity, grant.fencing_token)
        .await
        .err()
        .map(|err| err.to_string())
}

async fn release_unactivated_restore_grants(
    runtime: &IdentityRuntime,
    grants: &BTreeMap<AgentIdentity, LeaseGrant>,
    activated_identities: &BTreeSet<AgentIdentity>,
) -> Option<String> {
    let unactivated = grants
        .iter()
        .filter(|(identity, _)| !activated_identities.contains(*identity))
        .map(|(_, grant)| grant.clone())
        .collect::<Vec<_>>();
    if unactivated.is_empty() {
        return None;
    }
    match runtime.release_or_park_untracked_leases(&unactivated).await {
        Ok(()) => None,
        Err(error) => Some(error.to_string()),
    }
}

fn append_cleanup_error(message: String, cleanup_error: Option<String>) -> String {
    match cleanup_error {
        Some(cleanup_error) => format!("{message}; lease cleanup failed: {cleanup_error}"),
        None => message,
    }
}

type RosterAuthorityGuards = (
    tokio::sync::OwnedMutexGuard<()>,
    tokio::sync::OwnedMutexGuard<()>,
);

/// Reserve every identity and its public alias in one stable order before a
/// roster-wide continuity/lease decision. Keeping the guards owned lets the
/// restore work remain concurrent after the all-or-nothing authority gate,
/// while reset, delete, materialize, and raw-member creation cannot invalidate
/// any member's freshly resolved generation.
async fn acquire_roster_authority_guards(
    runtime: &IdentityRuntime,
    identities: &[AgentIdentity],
) -> Result<BTreeMap<AgentIdentity, RosterAuthorityGuards>, IdentityRuntimeError> {
    let ordered = identities.iter().cloned().collect::<BTreeSet<_>>();
    let mut lifecycle_guards = BTreeMap::new();
    for identity in &ordered {
        lifecycle_guards.insert(
            identity.clone(),
            runtime
                .lifecycle_lock_for(identity)
                .await
                .lock_owned()
                .await,
        );
    }

    let mut alias_guards = BTreeMap::new();
    for identity in &ordered {
        alias_guards.insert(
            identity.clone(),
            runtime
                .raw_member_alias_lock(identity.as_str())
                .await
                .lock_owned()
                .await,
        );
    }
    for identity in &ordered {
        runtime.ensure_raw_member_alias_available(identity).await?;
    }

    ordered
        .into_iter()
        .map(|identity| {
            let lifecycle = lifecycle_guards.remove(&identity).ok_or_else(|| {
                IdentityRuntimeError::Internal(format!(
                    "authority gate lost lifecycle reservation for {identity}"
                ))
            })?;
            let alias = alias_guards.remove(&identity).ok_or_else(|| {
                IdentityRuntimeError::Internal(format!(
                    "authority gate lost alias reservation for {identity}"
                ))
            })?;
            Ok((identity, (lifecycle, alias)))
        })
        .collect::<Result<_, IdentityRuntimeError>>()
}

// ---------------------------------------------------------------------------
// Restore flow orchestration — REQ-12
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestoreSnapshotPolicy {
    /// Preserve the historical `RestoreOutcome::Resumed.snapshot` payload.
    PreserveOutcomePayload,
    /// Bootstrap callers may omit the payload when the live bridge explicitly
    /// declares that resume is session-id-based.
    BridgeRequirementOnly,
}

fn should_load_resume_snapshot(
    policy: RestoreSnapshotPolicy,
    bridge_requires_snapshot: Option<bool>,
    already_active: bool,
) -> bool {
    // A reconcile pass over an already-active (converged) identity is
    // metadata convergence, never a second embodiment: no bridge call runs,
    // its outcome classification does not key on snapshot presence, and no
    // production consumer reads the payload. Loading it made every converged
    // reconcile pay one full session-blob read per member.
    if already_active {
        return false;
    }
    match policy {
        RestoreSnapshotPolicy::PreserveOutcomePayload => true,
        RestoreSnapshotPolicy::BridgeRequirementOnly => {
            // Without a bridge there is no explicit opt-out.
            bridge_requires_snapshot.unwrap_or(true)
        }
    }
}

/// Execute the full restore flow per REQ-12 sequencing.
///
/// Steps:
/// 1. Roster → identities
/// 2. Resolve continuity for all identities
/// 3. Acquire leases
/// 4. Compute topology (target activation set)
/// 5. Build context for each identity
/// 6. Run customizer
/// 7. Resume injection (for Ready)
/// 8. Lower to CreateSessionRequest (conceptual)
/// 9. Resume, 10. Create, 11. Fail per resolve state
///
/// When `bridge` is `Some`, Uninitialized identities call `bridge.create_session()`
/// and Ready identities call `bridge.resume_session()` to actually spawn/resume
/// mob members. When `None`, the flow runs validation-only (for tests).
pub async fn restore_flow(
    runtime: &IdentityRuntime,
    roster: &[DurableAgentSpec],
    topology_provider: Option<&dyn TopologyProvider>,
    customizer: Option<&dyn AgentCustomizer>,
) -> Result<RestoreFlowResult, IdentityRuntimeError> {
    restore_flow_with_snapshot_policy(
        runtime,
        roster,
        topology_provider,
        customizer,
        RestoreSnapshotPolicy::PreserveOutcomePayload,
    )
    .await
}

/// Execute restore for process bootstrap, avoiding an otherwise-unused
/// continuity snapshot read when the configured bridge explicitly declares
/// that it resumes by session id.
///
/// Like [`restore_flow`] since the idle-cost fix, a successful resumed
/// outcome may carry an empty snapshot: `restore_flow` elides the continuity
/// snapshot read for already-active identities even under
/// `RestoreSnapshotPolicy::PreserveOutcomePayload`. No `RestoreOutcome`
/// consumer may assume a populated `Resumed.snapshot`; read the durable
/// record instead when payload bytes are required.
pub async fn restore_flow_for_bootstrap(
    runtime: &IdentityRuntime,
    roster: &[DurableAgentSpec],
    topology_provider: Option<&dyn TopologyProvider>,
    customizer: Option<&dyn AgentCustomizer>,
) -> Result<RestoreFlowResult, IdentityRuntimeError> {
    restore_flow_with_snapshot_policy(
        runtime,
        roster,
        topology_provider,
        customizer,
        RestoreSnapshotPolicy::BridgeRequirementOnly,
    )
    .await
}

async fn restore_flow_with_snapshot_policy(
    runtime: &IdentityRuntime,
    roster: &[DurableAgentSpec],
    topology_provider: Option<&dyn TopologyProvider>,
    customizer: Option<&dyn AgentCustomizer>,
    snapshot_policy: RestoreSnapshotPolicy,
) -> Result<RestoreFlowResult, IdentityRuntimeError> {
    // INV-06: validate roster uniqueness before any work
    IdentityRuntime::validate_roster_uniqueness(roster)?;

    let identities: Vec<AgentIdentity> = roster.iter().map(|s| s.identity.clone()).collect();

    // Topology is declaration-only and does not require an embodiment lease.
    let topology_context = TopologyContext {
        roster: roster.to_vec(),
    };
    let managed_edges = if let Some(tp) = topology_provider {
        tp.compute_edges(&identities, &topology_context)
            .await
            .map_err(|error| IdentityRuntimeError::Internal(format!("topology: {error}")))?
    } else {
        Vec::new()
    };

    // Hold every lifecycle reservation before the fleet-wide read and lease
    // gate. This preserves the historical all-or-nothing ownership contract
    // without reopening the stale-generation race: each guard moves into the
    // corresponding concurrent restore future and remains held through that
    // member's explicit commit/rollback boundary.
    let mut authority_guards = acquire_roster_authority_guards(runtime, &identities).await?;
    runtime.release_parked_unactivated_leases().await?;
    let resolved = runtime
        .continuity_store()
        .resolve_many(&identities)
        .await
        .map_err(IdentityRuntimeError::Store)?;
    for identity in &identities {
        if !resolved.contains_key(identity) {
            return Err(IdentityRuntimeError::Internal(format!(
                "resolve_many did not return state for {identity}"
            )));
        }
    }
    // A live Active entry already owns exact provider authority. Reconcile is
    // metadata convergence for that member, not a second embodiment attempt;
    // reacquiring here either rotates the token needlessly or deadlocks strict
    // providers that correctly report the existing holder as AlreadyHeld.
    let mut already_active_identities = BTreeSet::new();
    for identity in &identities {
        if runtime.is_active(identity).await {
            already_active_identities.insert(identity.clone());
        }
    }
    let identities_to_acquire = identities
        .iter()
        .filter(|identity| !already_active_identities.contains(*identity))
        .cloned()
        .collect::<Vec<_>>();
    let lease_results = if identities_to_acquire.is_empty() {
        BTreeMap::new()
    } else {
        runtime
            .lease_provider()
            .acquire_leases(&identities_to_acquire, runtime.runtime_instance_id())
            .await
            .map_err(IdentityRuntimeError::Lease)?
    };
    let mut restore_grants = BTreeMap::new();
    let mut ownership_error = None;
    for identity in &identities_to_acquire {
        match lease_results.get(identity) {
            Some(LeaseAcquireResult::Acquired(grant)) => {
                restore_grants.insert(identity.clone(), grant.clone());
            }
            Some(LeaseAcquireResult::AlreadyHeld { holder, .. }) => {
                tracing::error!(
                    %identity,
                    holder = %holder,
                    "single-embodiment guard: restore refused — identity is already \
                     embodied by another live runtime instance"
                );
                ownership_error.get_or_insert_with(|| IdentityRuntimeError::AlreadyEmbodied {
                    identity: identity.clone(),
                    holder: holder.clone(),
                });
            }
            None => {
                ownership_error
                    .get_or_insert_with(|| IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        }
    }
    if let Some(error) = ownership_error {
        let cleanup_error =
            release_unactivated_restore_grants(runtime, &restore_grants, &BTreeSet::new()).await;
        return match cleanup_error {
            Some(cleanup_error) => Err(IdentityRuntimeError::Internal(append_cleanup_error(
                error.to_string(),
                Some(cleanup_error),
            ))),
            None => Err(error),
        };
    }

    let mut restore_work = Vec::with_capacity(roster.len());
    for (index, spec) in roster.iter().cloned().enumerate() {
        let identity = &spec.identity;
        let persisted_resolve_state = resolved.get(identity).cloned().ok_or_else(|| {
            IdentityRuntimeError::Internal(format!(
                "validated resolve state disappeared for {identity}"
            ))
        })?;
        let grant = if already_active_identities.contains(identity) {
            None
        } else {
            Some(restore_grants.remove(identity).ok_or_else(|| {
                IdentityRuntimeError::Internal(format!(
                    "validated restore grant disappeared for {identity}"
                ))
            })?)
        };
        let guards = authority_guards.remove(identity).ok_or_else(|| {
            IdentityRuntimeError::Internal(format!(
                "validated authority reservation disappeared for {identity}"
            ))
        })?;
        restore_work.push((index, spec, persisted_resolve_state, grant, guards));
    }

    // Steps 5-11: per-identity processing. Session restoration is dominated by
    // independent history loading and agent construction, so keep a small
    // bounded set in flight instead of making large durable rosters pay the
    // full sum of every member's resume latency.
    let restore_concurrency = identity_restore_concurrency();
    tracing::info!(
        member_count = roster.len(),
        concurrency = restore_concurrency,
        "starting identity restore"
    );
    let restore_started_at = Instant::now();
    let restore_results = stream::iter(restore_work)
        .map(|(index, spec, persisted_resolve_state, grant, authority_guards)| {
            let identities = identities.clone();
            let managed_edges = managed_edges.clone();
            let already_active_identities = already_active_identities.clone();
            async move {
                let _authority_guards = authority_guards;
                let member_started_at = Instant::now();
                let mut activated_identities = BTreeSet::new();
                let mut outcomes = BTreeMap::new();
                let spec = &spec;
                let identity = &spec.identity;
        let grants = grant
            .map(|grant| BTreeMap::from([(identity.clone(), grant)]))
            .unwrap_or_default();

        // If this identity is already registered and in Active state
        // (from a previous restore_flow call), skip bridge operations — the
        // mob member already exists. Identities in Retiring/Suspended state
        // need re-activation through the bridge.
        let already_active = already_active_identities.contains(identity);
        if already_active {
            let record = runtime.reuse_active_restore_state(spec).await?;
            // Converged pass: the member already exists, so the checkpoint
            // payload is inert metadata here (see should_load_resume_snapshot).
            let snapshot = SessionSnapshot { data: Vec::new() };
            let draft = AgentBuildDraft {
                model: None,
                system_prompt: None,
                additional_instructions: spec.additional_instructions.clone(),
                labels: spec.labels.clone(),
                app_context: spec.context.clone(),
                external_tools: Vec::new(),
                local_external_tools: Default::default(),
            };
            outcomes.insert(
                identity.clone(),
                RestoreOutcome::Resumed {
                    record,
                    snapshot,
                    draft,
                },
            );
            trace_identity_restore_completed(identity, member_started_at);
            return Ok((index, outcomes));
        }

        let resolve_state = if !already_active && durable_spec_uses_external_binding(spec) {
            ContinuityResolveState::Uninitialized
        } else {
            persisted_resolve_state
        };

        // Step 5: build context
        let build_context = AgentBuildContext {
            identity: identity.clone(),
            active_peers: identities.clone(),
            managed_edges: managed_edges.clone(),
            runtime_services: runtime.runtime_services(),
        };

        // Step 6: customize
        let mut draft = AgentBuildDraft {
            model: None,
            system_prompt: None,
            additional_instructions: spec.additional_instructions.clone(),
            labels: spec.labels.clone(),
            app_context: spec.context.clone(),
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
        };

        if let Some(cust) = customizer
            && let Err(e) = cust.customize_build(&build_context, spec, &mut draft).await
        {
            let cleanup_error =
                release_unactivated_restore_grants(runtime, &grants, &activated_identities).await;
            return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                format!("customizer: {e}"),
                cleanup_error,
            )));
        }

        match resolve_state {
            // Step 10: Uninitialized → fresh-create
            ContinuityResolveState::Uninitialized => {
                let new_runtime_id = match AgentRuntimeId::parse(&format!("rt:{identity}:0")) {
                    Ok(runtime_id) => runtime_id,
                    Err(e) => {
                        let cleanup_error = release_unactivated_restore_grants(
                            runtime,
                            &grants,
                            &activated_identities,
                        )
                        .await;
                        return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                            format!("failed to mint runtime id: {e}"),
                            cleanup_error,
                        )));
                    }
                };
                let new_session_id = meerkat_core::types::SessionId::new();
                let mut record = ContinuityRecord {
                    identity: identity.clone(),
                    agent_runtime_id: new_runtime_id,
                    session_id: new_session_id,
                    generation: ContinuityGeneration::new(0),
                    checkpoint_version: CheckpointVersion::new(0),
                };
                let initial_session_id = record.session_id.clone();
                let mut initial_record_persisted = false;

                // Persist the initial record before spawning through a
                // continuity-backed session store. PersistentSessionService
                // saves during member creation, and the store enforces CAS
                // against the continuity record.
                if let Some(grant) = grants.get(identity) {
                    if let Err(err) = runtime
                        .continuity_store()
                        .upsert_continuity_record(&record, grant.fencing_token)
                        .await
                    {
                        let cleanup_error = release_unactivated_restore_grants(
                            runtime,
                            &grants,
                            &activated_identities,
                        )
                        .await;
                        return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                            format!("continuity upsert before restore create: {err}"),
                            cleanup_error,
                        )));
                    }
                    initial_record_persisted = true;
                }

                // Bridge: create the real mob member when available.
                // Skip if the identity is already active (mob member exists).
                if !already_active && let Some(bridge) = runtime.bridge() {
                    if let Some(grant) = grants.get(identity)
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
                        let delete_error = delete_tentative_continuity_record(
                            runtime,
                            identity,
                            Some(grant),
                            initial_record_persisted,
                        )
                        .await;
                        let lease_cleanup_error = release_unactivated_restore_grants(
                            runtime,
                            &grants,
                            &activated_identities,
                        )
                        .await;
                        return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                            format!(
                                "bridge register_session_runtime_state: {err}{}",
                                delete_error
                                    .as_ref()
                                    .map(|e| format!("; tentative continuity cleanup failed: {e}"))
                                    .unwrap_or_default(),
                            ),
                            lease_cleanup_error,
                        )));
                    }
                    let session_id = match bridge
                        .create_session(
                            identity,
                            &record.agent_runtime_id,
                            spec,
                            &draft,
                            &record.session_id,
                        )
                        .await
                    {
                        Ok(session_id) => session_id,
                        Err(err) => {
                            let unregister_error = bridge
                                .unregister_session_runtime_state(&initial_session_id)
                                .await
                                .err();
                            let cleanup_error =
                                bridge.retire_member(&record.agent_runtime_id).await.err();
                            let delete_error = delete_tentative_continuity_record(
                                runtime,
                                identity,
                                grants.get(identity),
                                initial_record_persisted,
                            )
                            .await;
                            let lease_cleanup_error = release_unactivated_restore_grants(
                                runtime,
                                &grants,
                                &activated_identities,
                            )
                            .await;
                            return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                                format!(
                                    "bridge create_session: {err}{}{}{}",
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
                                        .map(|e| format!(
                                            "; tentative continuity cleanup failed: {e}"
                                        ))
                                        .unwrap_or_default(),
                                ),
                                lease_cleanup_error,
                            )));
                        }
                    };
                    // Update the record with the actual session ID from the mob
                    record.session_id = session_id;
                }
                if let Some(grant) = grants.get(identity)
                    && (!initial_record_persisted || record.session_id != initial_session_id)
                    && let Err(err) = runtime
                        .continuity_store()
                        .upsert_continuity_record(&record, grant.fencing_token)
                        .await
                {
                    let unregister_error = if let Some(bridge) = runtime.bridge() {
                        let mut sessions = vec![initial_session_id.clone()];
                        if record.session_id != initial_session_id {
                            sessions.push(record.session_id.clone());
                        }
                        let mut errors = Vec::new();
                        for session_id in sessions {
                            if let Err(err) =
                                bridge.unregister_session_runtime_state(&session_id).await
                            {
                                errors.push(format!("{session_id}: {err}"));
                            }
                        }
                        if errors.is_empty() {
                            None
                        } else {
                            Some(errors.join("; "))
                        }
                    } else {
                        None
                    };
                    let cleanup_error = if let Some(bridge) = runtime.bridge() {
                        bridge.retire_member(&record.agent_runtime_id).await.err()
                    } else {
                        None
                    };
                    let delete_error = delete_tentative_continuity_record(
                        runtime,
                        identity,
                        Some(grant),
                        initial_record_persisted,
                    )
                    .await;
                    let lease_cleanup_error =
                        release_unactivated_restore_grants(runtime, &grants, &activated_identities)
                            .await;
                    return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                        format!(
                            "continuity upsert after restore create: {err}{}{}{}",
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
                        ),
                        lease_cleanup_error,
                    )));
                }
                if let Some(grant) = grants.get(identity)
                    && let Some(bridge) = runtime.bridge()
                {
                    let effective_checkpoint_version = match bridge
                        .register_session_runtime_state(
                            &record.session_id,
                            identity,
                            record.generation,
                            record.checkpoint_version,
                            grant.fencing_token,
                        )
                        .await
                    {
                        Ok(version) => version,
                        Err(err) => {
                            let provisional_unregister_error = bridge
                                .unregister_session_runtime_state(&initial_session_id)
                                .await
                                .err();
                            let actual_unregister_error = if record.session_id == initial_session_id
                            {
                                None
                            } else {
                                bridge
                                    .unregister_session_runtime_state(&record.session_id)
                                    .await
                                    .err()
                            };
                            let cleanup_error =
                                bridge.retire_member(&record.agent_runtime_id).await.err();
                            let delete_error = delete_tentative_continuity_record(
                                runtime,
                                identity,
                                Some(grant),
                                initial_record_persisted,
                            )
                            .await;
                            let lease_cleanup_error = release_unactivated_restore_grants(
                                runtime,
                                &grants,
                                &activated_identities,
                            )
                            .await;
                            return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                                format!(
                                    "bridge register actual session runtime state: {err}{}{}{}{}",
                                    provisional_unregister_error
                                        .as_ref()
                                        .map(|e| format!("; unregister session failed: {e}"))
                                        .unwrap_or_default(),
                                    actual_unregister_error
                                        .as_ref()
                                        .map(|e| format!("; actual session unregister failed: {e}"))
                                        .unwrap_or_default(),
                                    cleanup_error
                                        .as_ref()
                                        .map(|e| format!("; cleanup retire failed: {e}"))
                                        .unwrap_or_default(),
                                    delete_error
                                        .as_ref()
                                        .map(|e| format!(
                                            "; tentative continuity cleanup failed: {e}"
                                        ))
                                        .unwrap_or_default(),
                                ),
                                lease_cleanup_error,
                            )));
                        }
                    };
                    record.checkpoint_version = effective_checkpoint_version;
                    if record.session_id != initial_session_id
                        && let Err(err) = bridge
                            .unregister_session_runtime_state(&initial_session_id)
                            .await
                    {
                        let actual_unregister_error = bridge
                            .unregister_session_runtime_state(&record.session_id)
                            .await
                            .err();
                        let cleanup_error =
                            bridge.retire_member(&record.agent_runtime_id).await.err();
                        let delete_error = delete_tentative_continuity_record(
                            runtime,
                            identity,
                            Some(grant),
                            initial_record_persisted,
                        )
                        .await;
                        let lease_cleanup_error = release_unactivated_restore_grants(
                            runtime,
                            &grants,
                            &activated_identities,
                        )
                        .await;
                        return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                            format!(
                                "bridge unregister abandoned session runtime state: {err}{}{}{}",
                                actual_unregister_error
                                    .as_ref()
                                    .map(|e| format!("; actual session: {e}"))
                                    .unwrap_or_default(),
                                cleanup_error
                                    .as_ref()
                                    .map(|e| format!("; cleanup retire failed: {e}"))
                                    .unwrap_or_default(),
                                delete_error
                                    .as_ref()
                                    .map(|e| format!("; tentative continuity cleanup failed: {e}"))
                                    .unwrap_or_default(),
                            ),
                            lease_cleanup_error,
                        )));
                    }
                }

                // Register in runtime
                runtime
                    .register(
                        spec.clone(),
                        IdentityLifecycleState::Active,
                        Some(record.clone()),
                        grants.get(identity).cloned(),
                    )
                    .await;
                activated_identities.insert(identity.clone());

                outcomes.insert(identity.clone(), RestoreOutcome::Created { record, draft });
            }

            // Step 9: Ready → resume from snapshot
            ContinuityResolveState::Ready { record } => {
                let mut record = record.clone();
                let previous_record = record.clone();
                // Step 7: public restore preserves the authoritative payload.
                // Bootstrap may skip the read only when a live bridge opts out
                // and will therefore provide the authoritative resume verdict.
                let bridge_requires_snapshot = runtime
                    .bridge()
                    .map(|bridge| bridge.requires_resume_snapshot());
                let snapshot = if should_load_resume_snapshot(
                    snapshot_policy,
                    bridge_requires_snapshot,
                    already_active,
                ) {
                    match runtime
                        .continuity_store()
                        .load_session_snapshot(&record.session_id)
                        .await
                    {
                        Ok(snapshot) => snapshot,
                        Err(err) => {
                            let cleanup_error = release_unactivated_restore_grants(
                                runtime,
                                &grants,
                                &activated_identities,
                            )
                            .await;
                            return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                                format!("load session snapshot before restore resume: {err}"),
                                cleanup_error,
                            )));
                        }
                    }
                } else {
                    None
                };
                let mut abandoned_session_registration = None;
                // The bridge's authoritative resume verdict, when a bridge ran:
                // `Some(true)` = the persisted session was resumed, `Some(false)`
                // = the bridge fresh-spawned (typed fallback). Reporting keys on
                // THIS, not on snapshot presence — reconcile must never report
                // `resumed` for a fresh-spawned member (the HomeCore lie).
                let mut bridge_resumed: Option<bool> = None;

                // Bridge: resume or create the real mob member when available.
                // Skip if the identity is already active (mob member exists).
                if !already_active && let Some(bridge) = runtime.bridge() {
                    if let Some(grant) = grants.get(identity) {
                        if let Err(err) = runtime
                            .continuity_store()
                            .upsert_continuity_record(&record, grant.fencing_token)
                            .await
                        {
                            let cleanup_error = release_unactivated_restore_grants(
                                runtime,
                                &grants,
                                &activated_identities,
                            )
                            .await;
                            return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                                format!("continuity upsert before restore resume: {err}"),
                                cleanup_error,
                            )));
                        }
                        if let Err(e) = bridge
                            .register_session_runtime_state(
                                &record.session_id,
                                identity,
                                record.generation,
                                record.checkpoint_version,
                                grant.fencing_token,
                            )
                            .await
                        {
                            let unregister_error = bridge
                                .unregister_session_runtime_state(&record.session_id)
                                .await
                                .err();
                            let cleanup_error = release_unactivated_restore_grants(
                                runtime,
                                &grants,
                                &activated_identities,
                            )
                            .await;
                            return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                                format!(
                                    "bridge register_session_runtime_state: {e}{}",
                                    unregister_error
                                        .as_ref()
                                        .map(|e| format!("; unregister session failed: {e}"))
                                        .unwrap_or_default(),
                                ),
                                cleanup_error,
                            )));
                        }
                    }
                    let registered_session_id = record.session_id.clone();
                    // When no checkpoint snapshot exists the session data still
                    // lives in the mob's session store; resume passes an empty
                    // snapshot and the bridge loads the persisted session by id.
                    let resume_snapshot = snapshot
                        .clone()
                        .unwrap_or(SessionSnapshot { data: Vec::new() });
                    let resumed_session_id = match bridge
                        .resume_session(
                            identity,
                            &record.agent_runtime_id,
                            spec,
                            &draft,
                            &record.session_id,
                            &resume_snapshot,
                        )
                        .await
                    {
                        Ok(outcome) => {
                            bridge_resumed = Some(outcome.fallback_reason().is_none());
                            outcome.session_id().clone()
                        }
                        Err(err) => {
                            // A rejected resume must not fail the whole restore
                            // flow, and must NEVER abandon the durable session —
                            // the transcript is the only copy. Keep the
                            // identity → session binding intact, surface the
                            // identity as Broken with the error attached, and
                            // let the next reconcile retry the resume. Cleanup
                            // is bookkeeping only: unregister the session
                            // runtime state registered above (retried next
                            // reconcile); do NOT retire the member (meerkat
                            // keeps Broken-in-roster restore diagnostics) and
                            // do NOT touch the continuity record.
                            tracing::error!(
                                %identity,
                                session_id = %registered_session_id,
                                error = %err,
                                "restore resume rejected; marking identity Broken and preserving \
                                 the durable session for reconcile retry"
                            );
                            if let Err(unregister_err) = bridge
                                .unregister_session_runtime_state(&registered_session_id)
                                .await
                            {
                                tracing::warn!(
                                    %identity,
                                    error = %unregister_err,
                                    "failed to unregister session runtime state after rejected resume"
                                );
                            }
                            runtime
                                .register(
                                    spec.clone(),
                                    IdentityLifecycleState::Broken,
                                    Some(record.clone()),
                                    None,
                                )
                                .await;
                            outcomes.insert(
                                identity.clone(),
                                RestoreOutcome::Broken(ContinuityFailure {
                                    identity: identity.clone(),
                                    kind: ContinuityFailureKind::ResumeRejected,
                                    record: Some(record.clone()),
                                    detail: err.to_string(),
                                }),
                            );
                            // Not added to activated_identities: release this
                            // identity's lease grant before completing its
                            // independently scheduled restore task.
                            if let Some(cleanup_error) = release_unactivated_restore_grants(
                                runtime,
                                &grants,
                                &activated_identities,
                            )
                            .await
                            {
                                return Err(IdentityRuntimeError::Internal(format!(
                                    "restore cleanup failed: {cleanup_error}"
                                )));
                            }
                            trace_identity_restore_completed(identity, member_started_at);
                            return Ok((index, outcomes));
                        }
                    };
                    record.session_id = resumed_session_id;
                    if record.session_id != registered_session_id {
                        abandoned_session_registration = Some(registered_session_id);
                    }
                }
                if grants.contains_key(identity) {
                    let resolved = match runtime
                        .continuity_store()
                        .resolve_many(std::slice::from_ref(identity))
                        .await
                    {
                        Ok(resolved) => resolved,
                        Err(err) => {
                            let (unregister_error, member_cleanup_error) =
                                if let Some(bridge) = runtime.bridge() {
                                    let mut sessions = Vec::new();
                                    if let Some(session_id) =
                                        abandoned_session_registration.as_ref()
                                    {
                                        sessions.push(session_id.clone());
                                    }
                                    if !sessions
                                        .iter()
                                        .any(|session_id| session_id == &record.session_id)
                                    {
                                        sessions.push(record.session_id.clone());
                                    }
                                    let mut unregister_errors = Vec::new();
                                    for session_id in sessions {
                                        if let Err(error) = bridge
                                            .unregister_session_runtime_state(&session_id)
                                            .await
                                        {
                                            unregister_errors.push(format!(
                                                "{session_id}: {error}"
                                            ));
                                        }
                                    }
                                    (
                                        (!unregister_errors.is_empty())
                                            .then(|| unregister_errors.join("; ")),
                                        bridge.retire_member(&record.agent_runtime_id).await.err(),
                                    )
                                } else {
                                    (None, None)
                                };
                            let lease_cleanup_error = release_unactivated_restore_grants(
                                runtime,
                                &grants,
                                &activated_identities,
                            )
                            .await;
                            return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                                format!(
                                    "resolve continuity before restore resume upsert: {err}{}{}",
                                    unregister_error
                                        .as_ref()
                                        .map(|error| format!(
                                            "; unregister session failed: {error}"
                                        ))
                                        .unwrap_or_default(),
                                    member_cleanup_error
                                        .as_ref()
                                        .map(|error| format!(
                                            "; cleanup retire failed: {error}"
                                        ))
                                        .unwrap_or_default(),
                                ),
                                lease_cleanup_error,
                            )));
                        }
                    };
                    if let Some(ContinuityResolveState::Ready {
                        record: current_record,
                    }) = resolved.get(identity)
                        && current_record.session_id == record.session_id
                        && current_record.generation == record.generation
                        && current_record.checkpoint_version > record.checkpoint_version
                    {
                        record.checkpoint_version = current_record.checkpoint_version;
                    }
                }
                if let Some(grant) = grants.get(identity)
                    && let Err(err) = runtime
                        .continuity_store()
                        .upsert_continuity_record(&record, grant.fencing_token)
                        .await
                {
                    let unregister_error = if let Some(bridge) = runtime.bridge() {
                        let mut sessions = Vec::new();
                        if let Some(session_id) = abandoned_session_registration.as_ref() {
                            sessions.push(session_id.clone());
                        }
                        if !sessions
                            .iter()
                            .any(|session_id| session_id == &record.session_id)
                        {
                            sessions.push(record.session_id.clone());
                        }
                        let mut errors = Vec::new();
                        for session_id in sessions {
                            if let Err(err) =
                                bridge.unregister_session_runtime_state(&session_id).await
                            {
                                errors.push(format!("{session_id}: {err}"));
                            }
                        }
                        if errors.is_empty() {
                            None
                        } else {
                            Some(errors.join("; "))
                        }
                    } else {
                        None
                    };
                    let cleanup_error = if let Some(bridge) = runtime.bridge() {
                        bridge.retire_member(&record.agent_runtime_id).await.err()
                    } else {
                        None
                    };
                    let lease_cleanup_error =
                        release_unactivated_restore_grants(runtime, &grants, &activated_identities)
                            .await;
                    return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                        format!(
                            "continuity upsert after restore resume: {err}{}{}",
                            unregister_error
                                .as_ref()
                                .map(|e| format!("; unregister session failed: {e}"))
                                .unwrap_or_default(),
                            cleanup_error
                                .as_ref()
                                .map(|e| format!("; cleanup retire failed: {e}"))
                                .unwrap_or_default(),
                        ),
                        lease_cleanup_error,
                    )));
                }
                if let Some(bridge) = runtime.bridge()
                    && let Some(grant) = grants.get(identity)
                {
                    let effective_checkpoint_version = match bridge
                        .register_session_runtime_state(
                            &record.session_id,
                            identity,
                            record.generation,
                            record.checkpoint_version,
                            grant.fencing_token,
                        )
                        .await
                    {
                        Ok(version) => version,
                        Err(err) => {
                            let abandoned_unregister_error =
                                if let Some(session_id) = abandoned_session_registration.as_ref() {
                                    bridge
                                        .unregister_session_runtime_state(session_id)
                                        .await
                                        .err()
                                } else {
                                    None
                                };
                            let actual_unregister_error = if abandoned_session_registration.as_ref()
                                == Some(&record.session_id)
                            {
                                None
                            } else {
                                bridge
                                    .unregister_session_runtime_state(&record.session_id)
                                    .await
                                    .err()
                            };
                            let cleanup_error =
                                bridge.retire_member(&record.agent_runtime_id).await.err();
                            let rollback_error = runtime
                                .continuity_store()
                                .upsert_continuity_record(&previous_record, grant.fencing_token)
                                .await
                                .err();
                            let lease_cleanup_error = release_unactivated_restore_grants(
                                runtime,
                                &grants,
                                &activated_identities,
                            )
                            .await;
                            return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                                format!(
                                    "bridge register_session_runtime_state: {err}{}{}{}{}",
                                    abandoned_unregister_error
                                        .as_ref()
                                        .map(|e| format!("; unregister session failed: {e}"))
                                        .unwrap_or_default(),
                                    actual_unregister_error
                                        .as_ref()
                                        .map(|e| format!("; actual session unregister failed: {e}"))
                                        .unwrap_or_default(),
                                    cleanup_error
                                        .as_ref()
                                        .map(|e| format!("; cleanup retire failed: {e}"))
                                        .unwrap_or_default(),
                                    rollback_error
                                        .as_ref()
                                        .map(|e| format!("; continuity rollback failed: {e}"))
                                        .unwrap_or_default(),
                                ),
                                lease_cleanup_error,
                            )));
                        }
                    };
                    record.checkpoint_version = effective_checkpoint_version;
                    if let Some(session_id) = abandoned_session_registration.as_ref()
                        && let Err(err) = bridge.unregister_session_runtime_state(session_id).await
                    {
                        let actual_unregister_error = bridge
                            .unregister_session_runtime_state(&record.session_id)
                            .await
                            .err();
                        let cleanup_error =
                            bridge.retire_member(&record.agent_runtime_id).await.err();
                        let rollback_error = runtime
                            .continuity_store()
                            .upsert_continuity_record(&previous_record, grant.fencing_token)
                            .await
                            .err();
                        let lease_cleanup_error = release_unactivated_restore_grants(
                            runtime,
                            &grants,
                            &activated_identities,
                        )
                        .await;
                        return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                            format!(
                                "bridge unregister abandoned session runtime state: {err}{}{}{}",
                                actual_unregister_error
                                    .as_ref()
                                    .map(|e| format!("; actual session: {e}"))
                                    .unwrap_or_default(),
                                cleanup_error
                                    .as_ref()
                                    .map(|e| format!("; cleanup retire failed: {e}"))
                                    .unwrap_or_default(),
                                rollback_error
                                    .as_ref()
                                    .map(|e| format!("; continuity rollback failed: {e}"))
                                    .unwrap_or_default(),
                            ),
                            lease_cleanup_error,
                        )));
                    }
                }

                // Register in runtime
                runtime
                    .register(
                        spec.clone(),
                        IdentityLifecycleState::Active,
                        Some(record.clone()),
                        grants.get(identity).cloned(),
                    )
                    .await;
                activated_identities.insert(identity.clone());

                // Outcome honesty: when a bridge ran, ITS verdict decides. A
                // typed fresh-spawn fallback reports `Created` — never
                // `Resumed` — regardless of snapshot presence; a successful
                // bridge resume reports `Resumed` even without a checkpoint
                // snapshot (the persisted mob session carried the history).
                // Without a bridge (metadata-only restore) the checkpoint
                // snapshot remains the only signal, as before.
                let resumed = match bridge_resumed {
                    Some(resumed) => resumed,
                    None => snapshot.is_some(),
                };
                if resumed {
                    outcomes.insert(
                        identity.clone(),
                        RestoreOutcome::Resumed {
                            record: record.clone(),
                            snapshot: snapshot.unwrap_or(SessionSnapshot { data: Vec::new() }),
                            draft,
                        },
                    );
                } else {
                    outcomes.insert(
                        identity.clone(),
                        RestoreOutcome::Created {
                            record: record.clone(),
                            draft,
                        },
                    );
                }
            }

            // Step 11: Broken → fail loudly (REQ-13)
            ContinuityResolveState::Broken { failure } => {
                // Keep a lifecycle projection even though no member was
                // materialized. The continuity repair supervisor discovers
                // Broken entries through the runtime roster; omitting this
                // entry made a transient eager-store failure terminal until a
                // manual reconcile or process restart.
                runtime
                    .register(
                        spec.clone(),
                        IdentityLifecycleState::Broken,
                        failure.record.clone(),
                        None,
                    )
                    .await;
                outcomes.insert(identity.clone(), RestoreOutcome::Broken(failure));
            }
        }
                if let Some(cleanup_error) =
                    release_unactivated_restore_grants(runtime, &grants, &activated_identities)
                        .await
                {
                    return Err(IdentityRuntimeError::Internal(format!(
                        "restore cleanup failed: {cleanup_error}"
                    )));
                }

                trace_identity_restore_completed(identity, member_started_at);
                Ok((index, outcomes))
            }
        })
        .buffer_unordered(restore_concurrency)
        .collect::<Vec<Result<(usize, BTreeMap<AgentIdentity, RestoreOutcome>), _>>>()
        .await;

    let mut ordered_results = Vec::with_capacity(restore_results.len());
    for result in restore_results {
        ordered_results.push(result?);
    }
    ordered_results.sort_by_key(|(index, _)| *index);

    let mut outcomes = BTreeMap::new();
    for (_, member_outcomes) in ordered_results {
        outcomes.extend(member_outcomes);
    }
    tracing::info!(
        member_count = roster.len(),
        elapsed_ms = restore_started_at.elapsed().as_millis(),
        "identity restore completed"
    );

    // Persist the provider declaration on eager restore too. Lazy bootstrap
    // already did this; without parity, later generation repair/materialize
    // read an empty desired set and could not reapply overlay topology.
    runtime.set_desired_peer_edges(managed_edges.clone()).await;
    if let Err(err) = runtime.reconcile_managed_peer_edges(&managed_edges).await {
        tracing::warn!(
            error = %err,
            "identity restore flow completed with topology reconcile warning"
        );
    }

    Ok(RestoreFlowResult {
        outcomes,
        managed_edges,
    })
}

/// Register the roster/topology metadata without materializing members.
///
/// This is the identity-first lazy bootstrap path: it validates the roster,
/// resolves cheap continuity metadata, records desired topology, and exposes
/// status/inspection surfaces. It deliberately does not acquire leases, call
/// customizers, load session snapshots, create sessions, or resume sessions.
pub async fn lazy_register_flow(
    runtime: &IdentityRuntime,
    roster: &[DurableAgentSpec],
    topology_provider: Option<&dyn TopologyProvider>,
) -> Result<RestoreFlowResult, IdentityRuntimeError> {
    IdentityRuntime::validate_roster_uniqueness(roster)?;

    let identities: Vec<AgentIdentity> = roster.iter().map(|s| s.identity.clone()).collect();

    let topology_context = TopologyContext {
        roster: roster.to_vec(),
    };
    let managed_edges = if let Some(tp) = topology_provider {
        tp.compute_edges(&identities, &topology_context)
            .await
            .map_err(|e| IdentityRuntimeError::Internal(format!("topology: {e}")))?
    } else {
        Vec::new()
    };
    runtime.set_desired_peer_edges(managed_edges.clone()).await;

    // Reserve the full roster before the one fleet-wide metadata read. Each
    // identity's guard remains held until its dormant/active projection is
    // published, so a queued reset/delete cannot invalidate the batch result
    // while preserving O(1) store round trips for large lazy rosters.
    let mut authority_guards = acquire_roster_authority_guards(runtime, &identities).await?;
    // Restore/materialization can fail before an IdentityEntry exists. Exact
    // grants from that phase live in the runtime-owned orphan ledger; both
    // eager and lazy reconcile must drain it before publishing a repair or
    // allowing later on-demand materialization to reacquire authority.
    runtime.release_parked_unactivated_leases().await?;
    let resolved = runtime
        .continuity_store()
        .resolve_many(&identities)
        .await
        .map_err(IdentityRuntimeError::Store)?;
    for identity in &identities {
        if !resolved.contains_key(identity) {
            return Err(IdentityRuntimeError::Internal(format!(
                "resolve_many did not return state for {identity}"
            )));
        }
    }

    let mut outcomes = BTreeMap::new();
    for spec in roster {
        let identity = &spec.identity;
        let _authority_guards = authority_guards.remove(identity).ok_or_else(|| {
            IdentityRuntimeError::Internal(format!(
                "validated lazy authority reservation disappeared for {identity}"
            ))
        })?;
        let resolve_state = resolved.get(identity).cloned().ok_or_else(|| {
            IdentityRuntimeError::Internal(format!(
                "validated lazy resolve state disappeared for {identity}"
            ))
        })?;
        let currently_active = runtime
            .status(identity)
            .await
            .is_ok_and(|status| status.state == IdentityLifecycleState::Active);
        let draft = AgentBuildDraft {
            model: None,
            system_prompt: None,
            additional_instructions: spec.additional_instructions.clone(),
            labels: spec.labels.clone(),
            app_context: spec.context.clone(),
            external_tools: Vec::new(),
            local_external_tools: Default::default(),
        };
        match resolve_state {
            ContinuityResolveState::Uninitialized => {
                if currently_active {
                    runtime.update_spec(spec.clone()).await?;
                } else {
                    runtime
                        .register(spec.clone(), IdentityLifecycleState::Dormant, None, None)
                        .await;
                }
                outcomes.insert(
                    identity.clone(),
                    RestoreOutcome::Dormant {
                        record: None,
                        draft,
                    },
                );
            }
            ContinuityResolveState::Ready { record } => {
                if currently_active {
                    runtime.update_spec(spec.clone()).await?;
                } else {
                    runtime
                        .register(
                            spec.clone(),
                            IdentityLifecycleState::Dormant,
                            Some(record.clone()),
                            None,
                        )
                        .await;
                }
                outcomes.insert(
                    identity.clone(),
                    RestoreOutcome::Dormant {
                        record: Some(record),
                        draft,
                    },
                );
            }
            ContinuityResolveState::Broken { failure } => {
                if matches!(failure.kind, ContinuityFailureKind::SnapshotMissing)
                    && let Some(record) = failure.record.clone()
                {
                    if currently_active {
                        runtime.update_spec(spec.clone()).await?;
                    } else {
                        runtime
                            .register(
                                spec.clone(),
                                IdentityLifecycleState::Dormant,
                                Some(record.clone()),
                                None,
                            )
                            .await;
                    }
                    outcomes.insert(
                        identity.clone(),
                        RestoreOutcome::Dormant {
                            record: Some(record),
                            draft,
                        },
                    );
                } else {
                    runtime
                        .register(
                            spec.clone(),
                            IdentityLifecycleState::Broken,
                            failure.record.clone(),
                            None,
                        )
                        .await;
                    outcomes.insert(identity.clone(), RestoreOutcome::Broken(failure));
                }
            }
        }
    }

    if let Err(err) = runtime.reconcile_managed_peer_edges(&managed_edges).await {
        tracing::warn!(
            error = %err,
            "identity lazy register flow completed with topology reconcile warning"
        );
    }

    Ok(RestoreFlowResult {
        outcomes,
        managed_edges,
    })
}
