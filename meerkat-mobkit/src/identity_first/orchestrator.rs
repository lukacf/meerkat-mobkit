//! Orchestrator: restore flow, reconciliation, and topology wiring.
//!
//! Implements REQ-12 (restore flow), REQ-13 (broken continuity), REQ-19/REQ-21
//! (topology reconciliation), REQ-20 (static wiring preserved), REQ-22 (topology
//! not continuity truth), and REQ-33 (identity-keyed reconciliation).

use std::collections::{BTreeMap, BTreeSet};

use super::contracts::{AgentCustomizer, TopologyProvider};
use super::runtime::{IdentityRuntime, IdentityRuntimeError};
use super::types::{
    AgentBuildContext, AgentBuildDraft, AgentIdentity, AgentRuntimeId, CheckpointVersion,
    ContinuityFailure, ContinuityFailureKind, ContinuityGeneration, ContinuityRecord,
    ContinuityResolveState, DurableAgentSpec, IdentityLifecycleState, LeaseAcquireResult,
    LeaseGrant, ManagedPeerEdge, SessionSnapshot, TopologyContext,
};

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
    runtime
        .lease_provider()
        .release_leases(&unactivated)
        .await
        .err()
        .map(|err| err.to_string())
}

fn append_cleanup_error(message: String, cleanup_error: Option<String>) -> String {
    match cleanup_error {
        Some(cleanup_error) => format!("{message}; lease cleanup failed: {cleanup_error}"),
        None => message,
    }
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

    // Step 3 is an ownership gate. Do not create or resume live members for
    // identities whose durable lease is held by another runtime.
    let mut grants: BTreeMap<AgentIdentity, LeaseGrant> = BTreeMap::new();
    for identity in &identities {
        match lease_results.get(identity) {
            Some(LeaseAcquireResult::Acquired(grant)) => {
                grants.insert(identity.clone(), grant.clone());
            }
            Some(LeaseAcquireResult::AlreadyHeld { .. }) | None => {
                let acquired = grants.values().cloned().collect::<Vec<_>>();
                if !acquired.is_empty() {
                    runtime
                        .lease_provider()
                        .release_leases(&acquired)
                        .await
                        .map_err(IdentityRuntimeError::Lease)?;
                }
                return Err(IdentityRuntimeError::NoActiveLease(identity.clone()));
            }
        }
    }

    // Step 4: topology reconciliation
    let topology_context = TopologyContext {
        roster: roster.to_vec(),
    };
    let mut activated_identities = BTreeSet::new();
    let managed_edges = if let Some(tp) = topology_provider {
        match tp.compute_edges(&identities, &topology_context).await {
            Ok(edges) => edges,
            Err(e) => {
                let cleanup_error =
                    release_unactivated_restore_grants(runtime, &grants, &activated_identities)
                        .await;
                return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                    format!("topology: {e}"),
                    cleanup_error,
                )));
            }
        }
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
        if already_active {
            let Some(grant) = grants.get(identity) else {
                let cleanup_error =
                    release_unactivated_restore_grants(runtime, &grants, &activated_identities)
                        .await;
                return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                    format!("missing lease grant for already-active identity {identity}"),
                    cleanup_error,
                )));
            };
            if let Err(err) = runtime.refresh_active_restore_grant(identity, grant).await {
                let cleanup_error =
                    release_unactivated_restore_grants(runtime, &grants, &activated_identities)
                        .await;
                return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                    format!("refresh active restore lease for {identity}: {err}"),
                    cleanup_error,
                )));
            }
            activated_identities.insert(identity.clone());
        }

        let persisted_resolve_state = resolved.get(identity).ok_or_else(|| {
            IdentityRuntimeError::Internal(format!(
                "resolve_many did not return state for {identity}"
            ))
        })?;
        let resolve_state = if !already_active && durable_spec_uses_external_binding(spec) {
            ContinuityResolveState::Uninitialized
        } else {
            persisted_resolve_state.clone()
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
                // Step 7: load snapshot for resume injection
                let snapshot = match runtime
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
                };
                let mut abandoned_session_registration = None;

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
                    let resumed_session_id = match &snapshot {
                        Some(snap) => match bridge
                            .resume_session(
                                identity,
                                &record.agent_runtime_id,
                                spec,
                                &draft,
                                &record.session_id,
                                snap,
                            )
                            .await
                        {
                            Ok(outcome) => outcome.session_id().clone(),
                            Err(err) => {
                                let unregister_error = bridge
                                    .unregister_session_runtime_state(&registered_session_id)
                                    .await
                                    .err();
                                let cleanup_error =
                                    bridge.retire_member(&record.agent_runtime_id).await.err();
                                let lease_cleanup_error = release_unactivated_restore_grants(
                                    runtime,
                                    &grants,
                                    &activated_identities,
                                )
                                .await;
                                return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                                    format!(
                                        "bridge resume_session: {err}{}{}",
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
                        },
                        None => {
                            // No snapshot but record exists — resume from session
                            // store using the session_id from the continuity record.
                            // The session data lives in the mob's session store,
                            // not the continuity store's snapshot.
                            match bridge
                                .resume_session(
                                    identity,
                                    &record.agent_runtime_id,
                                    spec,
                                    &draft,
                                    &record.session_id,
                                    &SessionSnapshot { data: Vec::new() },
                                )
                                .await
                            {
                                Ok(outcome) => outcome.session_id().clone(),
                                Err(err) => {
                                    let unregister_error = bridge
                                        .unregister_session_runtime_state(&registered_session_id)
                                        .await
                                        .err();
                                    let cleanup_error =
                                        bridge.retire_member(&record.agent_runtime_id).await.err();
                                    let lease_cleanup_error = release_unactivated_restore_grants(
                                        runtime,
                                        &grants,
                                        &activated_identities,
                                    )
                                    .await;
                                    return Err(IdentityRuntimeError::Internal(
                                        append_cleanup_error(
                                            format!(
                                                "bridge resume_session (no snapshot): {err}{}{}",
                                                unregister_error
                                                    .as_ref()
                                                    .map(|e| format!(
                                                        "; unregister session failed: {e}"
                                                    ))
                                                    .unwrap_or_default(),
                                                cleanup_error
                                                    .as_ref()
                                                    .map(|e| format!(
                                                        "; cleanup retire failed: {e}"
                                                    ))
                                                    .unwrap_or_default(),
                                            ),
                                            lease_cleanup_error,
                                        ),
                                    ));
                                }
                            }
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
                            let cleanup_error = release_unactivated_restore_grants(
                                runtime,
                                &grants,
                                &activated_identities,
                            )
                            .await;
                            return Err(IdentityRuntimeError::Internal(append_cleanup_error(
                                format!("resolve continuity before restore resume upsert: {err}"),
                                cleanup_error,
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

    if let Some(cleanup_error) =
        release_unactivated_restore_grants(runtime, &grants, &activated_identities).await
    {
        return Err(IdentityRuntimeError::Internal(format!(
            "restore cleanup failed: {cleanup_error}"
        )));
    }

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
    let resolved = runtime
        .continuity_store()
        .resolve_many(&identities)
        .await
        .map_err(IdentityRuntimeError::Store)?;

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

    let mut outcomes = BTreeMap::new();
    for spec in roster {
        let identity = &spec.identity;
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
        match resolved.get(identity).ok_or_else(|| {
            IdentityRuntimeError::Internal(format!(
                "resolve_many did not return state for {identity}"
            ))
        })? {
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
                        record: Some(record.clone()),
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
                    outcomes.insert(identity.clone(), RestoreOutcome::Broken(failure.clone()));
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
