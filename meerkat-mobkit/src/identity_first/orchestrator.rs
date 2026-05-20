//! Orchestrator: restore flow, reconciliation, and topology wiring.
//!
//! Implements REQ-12 (restore flow), REQ-13 (broken continuity), REQ-19/REQ-21
//! (topology reconciliation), REQ-20 (static wiring preserved), REQ-22 (topology
//! not continuity truth), and REQ-33 (identity-keyed reconciliation).

use std::collections::BTreeMap;

use super::contracts::{AgentCustomizer, TopologyProvider};
use super::runtime::{IdentityRuntime, IdentityRuntimeError};
use super::types::{
    AgentBuildContext, AgentBuildDraft, AgentIdentity, AgentRuntimeId, CheckpointVersion,
    ContinuityFailure, ContinuityGeneration, ContinuityRecord, ContinuityResolveState,
    DurableAgentSpec, IdentityLifecycleState, LeaseAcquireResult, LeaseGrant, ManagedPeerEdge,
    SessionSnapshot, TopologyContext,
};

// ---------------------------------------------------------------------------
// Restore flow result
// ---------------------------------------------------------------------------

/// Result of the restore flow for a single identity.
#[derive(Debug, Clone)]
pub enum RestoreOutcome {
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

// ---------------------------------------------------------------------------
// Restore flow orchestration — REQ-12
// ---------------------------------------------------------------------------

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
    // INV-06: validate roster uniqueness before any work
    IdentityRuntime::validate_roster_uniqueness(roster)?;

    let identities: Vec<AgentIdentity> = roster.iter().map(|s| s.identity.clone()).collect();

    // Step 2: resolve continuity
    let resolved = runtime
        .continuity_store()
        .resolve_many(&identities)
        .await
        .map_err(IdentityRuntimeError::Store)?;

    // Step 3: acquire leases for all identities
    let lease_results = runtime
        .lease_provider()
        .acquire_leases(&identities, runtime.runtime_instance_id())
        .await
        .map_err(IdentityRuntimeError::Lease)?;

    // Collect successful grants
    let mut grants: BTreeMap<AgentIdentity, LeaseGrant> = BTreeMap::new();
    for (id, result) in &lease_results {
        if let LeaseAcquireResult::Acquired(grant) = result {
            grants.insert(id.clone(), grant.clone());
        }
    }

    // Step 4: topology reconciliation
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

    // Steps 5-11: per-identity processing
    let mut outcomes = BTreeMap::new();
    for spec in roster {
        let identity = &spec.identity;

        // If this identity is already registered and in Active state
        // (from a previous restore_flow call), skip bridge operations — the
        // mob member already exists. Identities in Retiring/Suspended state
        // need re-activation through the bridge.
        let already_active = runtime.is_active(identity).await;

        let resolve_state = resolved.get(identity).ok_or_else(|| {
            IdentityRuntimeError::Internal(format!(
                "resolve_many did not return state for {identity}"
            ))
        })?;

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

        if let Some(cust) = customizer {
            cust.customize_build(&build_context, spec, &mut draft)
                .await
                .map_err(|e| IdentityRuntimeError::Internal(format!("customizer: {e}")))?;
        }

        match resolve_state {
            // Step 10: Uninitialized → fresh-create
            ContinuityResolveState::Uninitialized => {
                let new_runtime_id =
                    AgentRuntimeId::parse(&format!("rt:{identity}:0")).map_err(|e| {
                        IdentityRuntimeError::Internal(format!("failed to mint runtime id: {e}"))
                    })?;
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
                    runtime
                        .continuity_store()
                        .upsert_continuity_record(&record, grant.fencing_token)
                        .await?;
                    initial_record_persisted = true;
                }

                // Bridge: create the real mob member when available.
                // Skip if the identity is already active (mob member exists).
                if !already_active && let Some(bridge) = runtime.bridge() {
                    if let Some(grant) = grants.get(identity) {
                        bridge
                            .register_session_runtime_state(
                                &record.session_id,
                                identity,
                                record.generation,
                                record.checkpoint_version,
                                grant.fencing_token,
                            )
                            .await
                            .map_err(|e| {
                                IdentityRuntimeError::Internal(format!(
                                    "bridge register_session_runtime_state: {e}"
                                ))
                            })?;
                    }
                    let session_id = bridge
                        .create_session(
                            identity,
                            &record.agent_runtime_id,
                            spec,
                            &draft,
                            &record.session_id,
                        )
                        .await
                        .map_err(|e| {
                            IdentityRuntimeError::Internal(format!("bridge create_session: {e}"))
                        })?;
                    // Update the record with the actual session ID from the mob
                    record.session_id = session_id;
                }
                if let Some(grant) = grants.get(identity)
                    && (!initial_record_persisted || record.session_id != initial_session_id)
                {
                    runtime
                        .continuity_store()
                        .upsert_continuity_record(&record, grant.fencing_token)
                        .await?;
                }
                if let Some(grant) = grants.get(identity)
                    && let Some(bridge) = runtime.bridge()
                {
                    let effective_checkpoint_version = bridge
                        .register_session_runtime_state(
                            &record.session_id,
                            identity,
                            record.generation,
                            record.checkpoint_version,
                            grant.fencing_token,
                        )
                        .await
                        .map_err(|e| {
                            IdentityRuntimeError::Internal(format!(
                                "bridge register actual session runtime state: {e}"
                            ))
                        })?;
                    record.checkpoint_version = effective_checkpoint_version;
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

                outcomes.insert(identity.clone(), RestoreOutcome::Created { record, draft });
            }

            // Step 9: Ready → resume from snapshot
            ContinuityResolveState::Ready { record } => {
                let mut record = record.clone();
                // Step 7: load snapshot for resume injection
                let snapshot = runtime
                    .continuity_store()
                    .load_session_snapshot(&record.session_id)
                    .await
                    .map_err(IdentityRuntimeError::Store)?;

                // Bridge: resume or create the real mob member when available.
                // Skip if the identity is already active (mob member exists).
                if !already_active && let Some(bridge) = runtime.bridge() {
                    if let Some(grant) = grants.get(identity) {
                        bridge
                            .register_session_runtime_state(
                                &record.session_id,
                                identity,
                                record.generation,
                                record.checkpoint_version,
                                grant.fencing_token,
                            )
                            .await
                            .map_err(|e| {
                                IdentityRuntimeError::Internal(format!(
                                    "bridge register_session_runtime_state: {e}"
                                ))
                            })?;
                    }
                    let resumed_session_id = match &snapshot {
                        Some(snap) => bridge
                            .resume_session(
                                identity,
                                &record.agent_runtime_id,
                                spec,
                                &draft,
                                &record.session_id,
                                snap,
                            )
                            .await
                            .map_err(|e| {
                                IdentityRuntimeError::Internal(format!(
                                    "bridge resume_session: {e}"
                                ))
                            })?,
                        None => {
                            // No snapshot but record exists — resume from session
                            // store using the session_id from the continuity record.
                            // The session data lives in the mob's session store,
                            // not the continuity store's snapshot.
                            bridge
                                .resume_session(
                                    identity,
                                    &record.agent_runtime_id,
                                    spec,
                                    &draft,
                                    &record.session_id,
                                    &SessionSnapshot { data: Vec::new() },
                                )
                                .await
                                .map_err(|e| {
                                    IdentityRuntimeError::Internal(format!(
                                        "bridge resume_session (no snapshot): {e}"
                                    ))
                                })?
                        }
                    };
                    record.session_id = resumed_session_id;
                }
                if let Some(grant) = grants.get(identity) {
                    runtime
                        .continuity_store()
                        .upsert_continuity_record(&record, grant.fencing_token)
                        .await?;
                }
                if let Some(bridge) = runtime.bridge()
                    && let Some(grant) = grants.get(identity)
                {
                    let effective_checkpoint_version = bridge
                        .register_session_runtime_state(
                            &record.session_id,
                            identity,
                            record.generation,
                            record.checkpoint_version,
                            grant.fencing_token,
                        )
                        .await
                        .map_err(|e| {
                            IdentityRuntimeError::Internal(format!(
                                "bridge register_session_runtime_state: {e}"
                            ))
                        })?;
                    record.checkpoint_version = effective_checkpoint_version;
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

                match snapshot {
                    Some(snap) => {
                        outcomes.insert(
                            identity.clone(),
                            RestoreOutcome::Resumed {
                                record: record.clone(),
                                snapshot: snap,
                                draft,
                            },
                        );
                    }
                    None => {
                        // Record exists but no snapshot — treat as fresh create
                        // with existing record (first checkpoint hasn't happened yet)
                        outcomes.insert(
                            identity.clone(),
                            RestoreOutcome::Created {
                                record: record.clone(),
                                draft,
                            },
                        );
                    }
                }
            }

            // Step 11: Broken → fail loudly (REQ-13)
            ContinuityResolveState::Broken { failure } => {
                outcomes.insert(identity.clone(), RestoreOutcome::Broken(failure.clone()));
            }
        }
    }

    runtime.reconcile_managed_peer_edges(&managed_edges).await?;

    Ok(RestoreFlowResult {
        outcomes,
        managed_edges,
    })
}
