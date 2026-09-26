//! Background retirement for implicit delegation mobs.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use meerkat_core::{AgentExecutionSnapshot, TurnPhase};
use meerkat_mob::{AgentIdentity, MobMemberStatus};
use meerkat_mob_mcp::MobMcpState;

use crate::mob_handle_runtime::{
    DELEGATE_IDLE_RETIRE_DISABLED_LABEL, DELEGATE_IDLE_RETIRE_SECS_LABEL,
    DelegateIdleRetireOverride, ImplicitDelegateRetirementOverrides, MobRuntime,
};
use crate::runtime::RuntimeOptions;

use super::UnifiedRuntime;

impl UnifiedRuntime {
    pub(crate) async fn configure_implicit_delegate_retirement(&self, options: &RuntimeOptions) {
        let Some(state) = self.mob_runtime.agent_mob_mcp_state() else {
            return;
        };
        let sweep_interval =
            Duration::from_millis(options.implicit_delegate_idle_sweep_interval_ms.max(1_000));
        let overrides = self.mob_runtime.implicit_delegate_retirement_overrides();
        // Re-arm the sweep for members restored across a restart: their
        // opt-ins live in the durable metadata store, not in the roster.
        if let Some(overrides) = overrides.as_ref() {
            let restored = overrides
                .attach_durable_store(Arc::clone(&self.persistent_metadata))
                .await;
            if restored > 0 {
                tracing::info!(restored, "restored idle-retire opt-ins for spawned members");
            }
        }
        let task = tokio::spawn(run_implicit_delegate_retirement(
            self.mob_runtime.clone(),
            state,
            overrides,
            options
                .implicit_delegate_idle_retire_secs
                .map(Duration::from_secs),
            sweep_interval,
            Arc::clone(&self.implicit_delegate_identity_runtime),
        ));
        *self.implicit_delegate_retirement_task.lock().await = Some(task);
    }
}

async fn run_implicit_delegate_retirement(
    runtime: MobRuntime,
    state: Arc<MobMcpState>,
    per_delegate_overrides: Option<ImplicitDelegateRetirementOverrides>,
    default_idle_after: Option<Duration>,
    sweep_interval: Duration,
    identity_runtime: Arc<std::sync::RwLock<Option<Arc<crate::identity_first::IdentityRuntime>>>>,
) {
    let primary_mob_id = runtime.handle().mob_id().to_string();
    let session_service = state.session_service();
    let mut idle_since: BTreeMap<(String, String), Instant> = BTreeMap::new();

    loop {
        tokio::time::sleep(sweep_interval).await;
        let mut seen = BTreeSet::new();
        // Opt-ins for members of implicit delegation mobs, as held before
        // this pass looked at any roster. Their mobs are not on the primary
        // event stream, so a hand retire or a destroy there is settled here:
        // an opt-in recorded before the pass whose member the pass did not
        // find is released.
        let implicit_opt_ins_at_start = match per_delegate_overrides.as_ref() {
            Some(overrides) => overrides.bindings_outside(&primary_mob_id).await,
            None => Vec::new(),
        };
        let mob_handles = Box::pin(state.mob_handles_snapshot()).await.ok();
        let rosters_observed = mob_handles.is_some();
        for (mob_id, handle) in mob_handles.unwrap_or_default() {
            let is_primary_mob = mob_id.as_str() == primary_mob_id;
            let is_implicit_mob = Box::pin(state.is_implicit_mob(&mob_id)).await;
            if !is_primary_mob && !is_implicit_mob {
                continue;
            }
            let members = handle.list_members_observation_snapshot().await;
            let retiring: BTreeSet<AgentIdentity> = members
                .iter()
                .filter(|member| member.status == MobMemberStatus::Retiring)
                .map(|member| member.agent_identity.clone())
                .collect();
            for member in members {
                let identity = member.agent_identity.to_string();
                let key = (mob_id.to_string(), identity.clone());
                seen.insert(key.clone());
                if member.status == MobMemberStatus::Retiring {
                    idle_since.remove(&key);
                    continue;
                }
                // An opt-in holds only for the member instance it was set
                // for; the overrides drop a stale binding (another member's)
                // and restore an opt-in released with the exact session the
                // member resumed onto, against the member's session now.
                let member_session = handle
                    .resolve_bridge_session_id(&member.agent_identity)
                    .await;
                let per_delegate_override = match per_delegate_overrides.as_ref() {
                    Some(overrides) => {
                        overrides
                            .reconcile_seated(mob_id.as_str(), &identity, member_session.as_ref())
                            .await
                    }
                    None => None,
                };
                if !idle_retirement_candidate(
                    is_primary_mob,
                    is_implicit_mob,
                    &member.labels,
                    per_delegate_override,
                ) {
                    idle_since.remove(&key);
                    continue;
                }
                let Some(idle_after) = delegate_member_idle_retire_after(
                    &member.labels,
                    per_delegate_override,
                    default_idle_after,
                ) else {
                    idle_since.remove(&key);
                    continue;
                };
                let Some(session_id) = member_session else {
                    idle_since.remove(&key);
                    continue;
                };
                let idle = match session_service.execution_snapshot(&session_id).await {
                    Ok(Some(snapshot)) => delegate_execution_is_idle(&snapshot),
                    Ok(None) => true,
                    Err(error) => {
                        tracing::debug!(
                            mob_id = %mob_id,
                            agent_identity = %identity,
                            session_id = %session_id,
                            error = %error,
                            "implicit delegate idle sweep skipped member after snapshot error"
                        );
                        false
                    }
                };
                if !idle {
                    idle_since.remove(&key);
                    continue;
                }
                // Retiring a member retires every member it spawned (meerkat
                // cascades along roster `spawned_by`). A member that owns a
                // live member, directly or through its children, is not idle:
                // a fork it started may still be running, and retiring it
                // would kill that fork with its outcome reaching nobody.
                let roster = handle.list_all_members().await;
                if spawned_subtree_has_live_member(
                    &member.agent_identity,
                    roster.iter().filter_map(|entry| {
                        Some((&entry.agent_identity, entry.spawned_by.as_ref()?))
                    }),
                    &retiring,
                ) {
                    idle_since.remove(&key);
                    continue;
                }
                let since = idle_since.entry(key.clone()).or_insert_with(Instant::now);
                if since.elapsed() < idle_after {
                    continue;
                }
                let public_alias =
                    crate::member_comms_id::runtime_alias_str(&identity).into_owned();
                let identity_runtime = identity_runtime
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone();
                let retire_result = if is_primary_mob {
                    if let Some(identity_runtime) = identity_runtime.as_ref() {
                        if let Some(durable_identity) = identity_runtime
                            .identity_for_member_mutation(&public_alias)
                            .await
                        {
                            identity_runtime
                                .retire_member_alias_tracked(&durable_identity, &public_alias)
                                .await
                                .map(|_| ())
                                .map_err(|error| error.to_string())
                        } else if crate::member_comms_id::is_reserved_generated_alias(&public_alias)
                        {
                            Err(format!(
                                "generated alias is not owned by IdentityRuntime: {public_alias}"
                            ))
                        } else {
                            handle
                                .retire(AgentIdentity::from(identity.as_str()))
                                .await
                                .map_err(|error| error.to_string())
                        }
                    } else if crate::member_comms_id::is_reserved_generated_alias(&public_alias) {
                        Err(format!(
                            "generated alias requires IdentityRuntime authority: {public_alias}"
                        ))
                    } else {
                        handle
                            .retire(AgentIdentity::from(identity.as_str()))
                            .await
                            .map_err(|error| error.to_string())
                    }
                } else if crate::member_comms_id::is_reserved_generated_alias(&public_alias) {
                    Err(format!(
                        "generated alias cannot be raw-retired in an implicit mob: {public_alias}"
                    ))
                } else {
                    handle
                        .retire(AgentIdentity::from(identity.as_str()))
                        .await
                        .map_err(|error| error.to_string())
                };
                match retire_result {
                    Ok(()) => {
                        // Exact on the session just retired; covers implicit
                        // mobs, whose events this runtime does not observe.
                        if let Some(overrides) = per_delegate_overrides.as_ref() {
                            overrides
                                .release_session(mob_id.as_str(), &identity, &session_id)
                                .await;
                        }
                        tracing::info!(
                            mob_id = %mob_id,
                            agent_identity = %identity,
                            idle_after_ms = idle_after.as_millis() as u64,
                            "retired idle spawned member"
                        );
                    }
                    Err(error) => {
                        tracing::debug!(
                            mob_id = %mob_id,
                            agent_identity = %identity,
                            error = %error,
                            "implicit delegate idle retirement failed"
                        );
                    }
                }
                idle_since.remove(&key);
            }
        }
        idle_since.retain(|key, _| seen.contains(key));
        if rosters_observed && let Some(overrides) = per_delegate_overrides.as_ref() {
            for ((mob_id, member_id), bound) in implicit_opt_ins_at_start {
                // A member being respawned can be missing from the roster for
                // a moment; its respawn carries the opt-in, so it is skipped.
                if !seen.contains(&(mob_id.clone(), member_id.clone()))
                    && !overrides.is_respawning(&mob_id, &member_id)
                {
                    overrides.clear(&mob_id, &member_id, &bound).await;
                }
            }
        }
    }
}

fn delegate_execution_is_idle(snapshot: &AgentExecutionSnapshot) -> bool {
    turn_phase_is_idle(snapshot.turn_phase)
}

fn idle_retirement_candidate(
    is_primary_mob: bool,
    is_implicit_mob: bool,
    labels: &std::collections::BTreeMap<String, String>,
    per_delegate_override: Option<DelegateIdleRetireOverride>,
) -> bool {
    if is_implicit_mob {
        return true;
    }
    is_primary_mob
        && (per_delegate_override.is_some() || labels.contains_key(DELEGATE_IDLE_RETIRE_SECS_LABEL))
}

fn delegate_member_idle_retire_after(
    labels: &std::collections::BTreeMap<String, String>,
    per_delegate_override: Option<DelegateIdleRetireOverride>,
    default_idle_after: Option<Duration>,
) -> Option<Duration> {
    match per_delegate_override {
        Some(DelegateIdleRetireOverride::Disabled) => return None,
        Some(DelegateIdleRetireOverride::Seconds(seconds)) => {
            return Some(Duration::from_secs(seconds));
        }
        // Opted in without a number: the runtime default decides, and a
        // runtime with retirement disabled keeps it disabled.
        Some(DelegateIdleRetireOverride::RuntimeDefault) => return default_idle_after,
        None => {}
    }
    match labels
        .get(DELEGATE_IDLE_RETIRE_SECS_LABEL)
        .map(String::as_str)
    {
        Some(value) if value.eq_ignore_ascii_case(DELEGATE_IDLE_RETIRE_DISABLED_LABEL) => None,
        Some(value) => value
            .parse::<u64>()
            .ok()
            .map(Duration::from_secs)
            .or(default_idle_after),
        None => default_idle_after,
    }
}

pub(crate) fn turn_phase_is_idle(phase: TurnPhase) -> bool {
    // Meerkat 0.7 removed `TurnPhase::is_terminal`; idle means ready or any
    // terminal phase (completed/failed/cancelled, matching the old predicate).
    matches!(
        phase,
        TurnPhase::Ready | TurnPhase::Completed | TurnPhase::Failed | TurnPhase::Cancelled
    )
}

/// Whether `member` spawned a member that is still live, directly or through
/// members it spawned. `spawned` lists each spawned member with its spawner
/// (roster `spawned_by`); exactly these descendants go when `member` is
/// retired. Every descendant not in `retiring` counts as live, including one
/// seated after the sweep's member snapshot was taken, and a retiring
/// descendant's own children are still looked at.
fn spawned_subtree_has_live_member<'a>(
    member: &'a AgentIdentity,
    spawned: impl Iterator<Item = (&'a AgentIdentity, &'a AgentIdentity)>,
    retiring: &BTreeSet<AgentIdentity>,
) -> bool {
    let mut children: BTreeMap<&AgentIdentity, Vec<&AgentIdentity>> = BTreeMap::new();
    for (child, spawner) in spawned {
        children.entry(spawner).or_default().push(child);
    }
    let mut visited = BTreeSet::from([member]);
    let mut owners = vec![member];
    while let Some(owner) = owners.pop() {
        for &child in children.get(owner).into_iter().flatten() {
            if !visited.insert(child) {
                continue;
            }
            if !retiring.contains(child) {
                return true;
            }
            owners.push(child);
        }
    }
    false
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn implicit_delegate_turn_phase_idle_classification() {
        assert!(turn_phase_is_idle(TurnPhase::Ready));
        assert!(turn_phase_is_idle(TurnPhase::Completed));
        assert!(turn_phase_is_idle(TurnPhase::Failed));
        assert!(turn_phase_is_idle(TurnPhase::Cancelled));

        assert!(!turn_phase_is_idle(TurnPhase::ApplyingPrimitive));
        assert!(!turn_phase_is_idle(TurnPhase::CallingLlm));
        assert!(!turn_phase_is_idle(TurnPhase::WaitingForOps));
        assert!(!turn_phase_is_idle(TurnPhase::DrainingBoundary));
        assert!(!turn_phase_is_idle(TurnPhase::Extracting));
        assert!(!turn_phase_is_idle(TurnPhase::ErrorRecovery));
        assert!(!turn_phase_is_idle(TurnPhase::Cancelling));
    }

    #[test]
    fn idle_retirement_candidates_require_primary_mob_opt_in() {
        let no_labels = std::collections::BTreeMap::new();
        let labeled = std::collections::BTreeMap::from([(
            DELEGATE_IDLE_RETIRE_SECS_LABEL.to_string(),
            "300".to_string(),
        )]);

        assert!(idle_retirement_candidate(false, true, &no_labels, None,));
        assert!(idle_retirement_candidate(true, false, &labeled, None,));
        assert!(idle_retirement_candidate(
            true,
            false,
            &no_labels,
            Some(DelegateIdleRetireOverride::Seconds(60)),
        ));
        assert!(idle_retirement_candidate(
            true,
            false,
            &no_labels,
            Some(DelegateIdleRetireOverride::RuntimeDefault),
        ));
        assert!(!idle_retirement_candidate(true, false, &no_labels, None,));
    }

    /// A fork child opted in on the runtime default follows that default
    /// exactly: the configured timeout when there is one, and no retirement
    /// when the runtime disabled it. A label never overrides the opt-in.
    #[test]
    fn runtime_default_override_follows_the_runtime_default() {
        let no_labels = std::collections::BTreeMap::new();
        assert_eq!(
            delegate_member_idle_retire_after(
                &no_labels,
                Some(DelegateIdleRetireOverride::RuntimeDefault),
                Some(Duration::from_mins(5))
            ),
            Some(Duration::from_mins(5))
        );
        assert_eq!(
            delegate_member_idle_retire_after(
                &no_labels,
                Some(DelegateIdleRetireOverride::RuntimeDefault),
                None
            ),
            None
        );
        let labels = std::collections::BTreeMap::from([(
            DELEGATE_IDLE_RETIRE_SECS_LABEL.to_string(),
            "12".to_string(),
        )]);
        assert_eq!(
            delegate_member_idle_retire_after(
                &labels,
                Some(DelegateIdleRetireOverride::RuntimeDefault),
                Some(Duration::from_mins(5))
            ),
            Some(Duration::from_mins(5))
        );
    }

    #[test]
    fn implicit_delegate_idle_retire_label_overrides_runtime_default() {
        let labels = std::collections::BTreeMap::from([(
            DELEGATE_IDLE_RETIRE_SECS_LABEL.to_string(),
            "12".to_string(),
        )]);

        assert_eq!(
            delegate_member_idle_retire_after(&labels, None, Some(Duration::from_mins(5))),
            Some(Duration::from_secs(12))
        );
    }

    #[test]
    fn implicit_delegate_idle_retire_label_can_disable_member_retirement() {
        let labels = std::collections::BTreeMap::from([(
            DELEGATE_IDLE_RETIRE_SECS_LABEL.to_string(),
            DELEGATE_IDLE_RETIRE_DISABLED_LABEL.to_string(),
        )]);

        assert_eq!(
            delegate_member_idle_retire_after(&labels, None, Some(Duration::from_mins(5))),
            None
        );
    }

    #[test]
    fn implicit_delegate_idle_retire_call_override_wins_over_label() {
        let labels = std::collections::BTreeMap::from([(
            DELEGATE_IDLE_RETIRE_SECS_LABEL.to_string(),
            "12".to_string(),
        )]);

        assert_eq!(
            delegate_member_idle_retire_after(
                &labels,
                Some(DelegateIdleRetireOverride::Seconds(9)),
                Some(Duration::from_mins(5))
            ),
            Some(Duration::from_secs(9))
        );
    }

    #[test]
    fn implicit_delegate_idle_retire_call_override_can_disable_retirement() {
        assert_eq!(
            delegate_member_idle_retire_after(
                &std::collections::BTreeMap::new(),
                Some(DelegateIdleRetireOverride::Disabled),
                Some(Duration::from_mins(5))
            ),
            None
        );
    }

    #[test]
    fn implicit_delegate_idle_retire_invalid_label_uses_runtime_default() {
        let labels = std::collections::BTreeMap::from([(
            DELEGATE_IDLE_RETIRE_SECS_LABEL.to_string(),
            "eventually".to_string(),
        )]);

        assert_eq!(
            delegate_member_idle_retire_after(&labels, None, Some(Duration::from_mins(5))),
            Some(Duration::from_mins(5))
        );
    }

    #[test]
    fn implicit_delegate_idle_retire_uses_runtime_default_when_unlabeled() {
        assert_eq!(
            delegate_member_idle_retire_after(
                &std::collections::BTreeMap::new(),
                None,
                Some(Duration::from_mins(5))
            ),
            Some(Duration::from_mins(5))
        );
    }

    #[tokio::test]
    async fn running_sweeper_observes_identity_authority_attached_after_runtime_bootstrap() {
        use crate::identity_first::{
            AgentAddressability, AgentIdentity as DurableIdentity, AgentRuntimeId,
            CheckpointVersion, ContinuityGeneration, ContinuityRecord, ContinuityStore,
            DurabilityPolicy, DurableAgentSpec, IdentityFirstRuntimeContext,
            IdentityLifecycleState, IdentityRuntime, IdentityRuntimeConfig, LeaseAcquireResult,
            LeaseProvider, LocalContinuityStore, LocalLeaseProvider, MobSessionBridge,
            RosterContext, RosterError, RosterProvider,
        };
        use crate::{
            DiscoverySpec, InMemoryMetadataStore, MobBootstrapOptions, MobBootstrapSpec,
            MobKitConfig,
        };

        struct FixedRoster(Vec<DurableAgentSpec>);

        #[async_trait::async_trait]
        impl RosterProvider for FixedRoster {
            async fn roster(
                &self,
                _context: &RosterContext,
            ) -> Result<Vec<DurableAgentSpec>, RosterError> {
                Ok(self.0.clone())
            }
        }

        let temp = tempfile::tempdir().expect("tempdir");
        let definition = meerkat_mob::MobDefinition::from_toml(
            r#"
[mob]
id = "late-bound-retirement"

[profiles.worker]
model = "gpt-5.5"
external_addressable = true

[profiles.worker.tools]
comms = true
"#,
        )
        .expect("mob definition");
        let mob_spec = MobBootstrapSpec::ephemeral(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            temp.path().to_path_buf(),
            4,
            None,
        )
        .with_options(MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(meerkat_client::TestClient::default())),
        });
        let mut runtime = UnifiedRuntime::bootstrap_with_options(
            mob_spec,
            MobKitConfig {
                modules: Vec::new(),
                discovery: DiscoverySpec {
                    namespace: "late-bound-retirement".to_string(),
                    modules: Vec::new(),
                },
                pre_spawn: Vec::new(),
            },
            Vec::new(),
            Duration::from_secs(2),
            RuntimeOptions {
                implicit_delegate_idle_retire_secs: Some(300),
                implicit_delegate_idle_sweep_interval_ms: 1_000,
                ..RuntimeOptions::default()
            },
            Arc::new(InMemoryMetadataStore::new()),
        )
        .await
        .expect("bootstrap runtime");
        assert!(runtime.identity_runtime().is_none());
        assert!(
            runtime
                .implicit_delegate_retirement_task
                .lock()
                .await
                .is_some(),
            "the sweeper must be running before identity authority attaches"
        );

        let identity = DurableIdentity::parse("domain:late-bound").expect("identity");
        let alias = "rt:domain:late-bound:0";
        let roster_member = crate::member_comms_id::mob_member_id(alias);
        let handle = runtime.mob_handle();
        handle
            .ensure_member(
                meerkat_mob::SpawnMemberSpec::new(
                    meerkat_mob::ProfileName::from("worker"),
                    roster_member.clone(),
                )
                .with_labels(BTreeMap::from([(
                    DELEGATE_IDLE_RETIRE_SECS_LABEL.to_string(),
                    "0".to_string(),
                )])),
            )
            .await
            .expect("spawn pre-attached generated member");
        let session_id = handle
            .resolve_bridge_session_id_observation(&roster_member)
            .await
            .expect("member session id");

        let continuity_store =
            Arc::new(LocalContinuityStore::in_memory().expect("late-bound continuity store"));
        let lease_provider = Arc::new(LocalLeaseProvider::new());
        let leases = lease_provider
            .acquire_leases(std::slice::from_ref(&identity), "late-bound-retirement")
            .await
            .expect("identity lease");
        let lease = match leases.get(&identity) {
            Some(LeaseAcquireResult::Acquired(lease)) => lease.clone(),
            other => panic!("expected acquired lease, got {other:?}"),
        };
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse(alias).expect("runtime alias"),
            session_id,
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        continuity_store
            .upsert_continuity_record(&record, lease.fencing_token)
            .await
            .expect("persist continuity");
        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store,
            lease_provider,
            runtime_instance_id: "late-bound-retirement".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(Arc::new(MobSessionBridge::new(handle))),
            default_timeout: None,
        }));
        let spec = DurableAgentSpec {
            identity: identity.clone(),
            profile: meerkat_mob::ProfileName::from("worker"),
            addressability: AgentAddressability::Addressable,
            display_name: None,
            labels: BTreeMap::new(),
            context: None,
            additional_instructions: Vec::new(),
            initial_message: None,
            runtime_mode_override: None,
            backend: None,
            binding: None,
            placement: None,
        };
        identity_runtime
            .register(
                spec.clone(),
                IdentityLifecycleState::Active,
                Some(record),
                Some(lease),
            )
            .await;
        runtime.attach_identity_first_context(Arc::new(IdentityFirstRuntimeContext::new(
            Arc::clone(&identity_runtime),
            Arc::new(FixedRoster(vec![spec])),
            None,
            None,
            Some(runtime.mob_handle().definition().clone()),
        )));

        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if identity_runtime
                    .status(&identity)
                    .await
                    .is_ok_and(|status| status.state == IdentityLifecycleState::Retiring)
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("late-bound sweeper never retired through identity authority");

        runtime.shutdown().await;
    }

    fn bound(
        policy: DelegateIdleRetireOverride,
        session_id: Option<&meerkat_core::types::SessionId>,
    ) -> crate::mob_handle_runtime::BoundIdleRetireOverride {
        crate::mob_handle_runtime::BoundIdleRetireOverride {
            policy,
            session_id: session_id.cloned(),
            recorded_at: chrono::Utc::now(),
        }
    }

    /// The policy and session an opt-in holds, for comparisons that do not
    /// care when it was recorded.
    fn view(
        bound: Option<crate::mob_handle_runtime::BoundIdleRetireOverride>,
    ) -> Option<(
        DelegateIdleRetireOverride,
        Option<meerkat_core::types::SessionId>,
    )> {
        bound.map(|bound| (bound.policy, bound.session_id))
    }

    fn record(
        mob_id: &str,
        member_id: &str,
        session_id: meerkat_core::types::SessionId,
        policy: DelegateIdleRetireOverride,
    ) -> crate::MemberIdleRetireOverrideRecord {
        crate::MemberIdleRetireOverrideRecord {
            mob_id: mob_id.to_string(),
            member_id: member_id.to_string(),
            session_id,
            policy,
            recorded_at: chrono::Utc::now(),
        }
    }

    /// The attach restores the bound opt-ins a previous process recorded and
    /// persists the bound opt-ins set before it (unbound ones, which only
    /// standalone wiring produces, are never persisted).
    #[tokio::test]
    async fn attach_restores_bound_opt_ins_and_persists_earlier_ones() {
        use crate::{PersistentMetadataStore, SqliteMetadataStore};
        use meerkat_core::types::SessionId;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("metadata.sqlite3");
        let (previous, early) = (SessionId::new(), SessionId::new());
        SqliteMetadataStore::open(&path)
            .expect("open metadata store")
            .set_member_idle_retire_override(&record(
                "mob-a",
                "fork-child",
                previous.clone(),
                DelegateIdleRetireOverride::Seconds(300),
            ))
            .await
            .expect("previous process row");

        let overrides = ImplicitDelegateRetirementOverrides::default();
        overrides
            .insert_for_test(
                "mob-a",
                "early-fork",
                bound(DelegateIdleRetireOverride::RuntimeDefault, Some(&early)),
            )
            .await;
        overrides
            .insert_for_test(
                "mob-a",
                "unbound",
                bound(DelegateIdleRetireOverride::Seconds(1), None),
            )
            .await;
        let restored = overrides
            .attach_durable_store(Arc::new(
                SqliteMetadataStore::open(&path).expect("reopen metadata store"),
            ))
            .await;
        assert_eq!(restored, 1);
        assert_eq!(
            view(overrides.get_bound("mob-a", "fork-child").await),
            Some((
                DelegateIdleRetireOverride::Seconds(300),
                Some(previous.clone())
            ))
        );

        let persisted = SqliteMetadataStore::open(&path)
            .expect("probe metadata store")
            .load_member_idle_retire_overrides()
            .await
            .expect("load");
        let members: Vec<&str> = persisted.iter().map(|r| r.member_id.as_str()).collect();
        assert_eq!(members, vec!["early-fork", "fork-child"]);
    }

    /// An unreadable store restores nothing but still receives the opt-ins
    /// set before the attach.
    #[tokio::test]
    async fn attach_to_an_unreadable_store_still_persists_earlier_opt_ins() {
        use crate::{MemberIdleRetireOverrideRecord, MetadataStoreError, PersistentMetadataStore};
        use meerkat_core::types::SessionId;

        #[derive(Default)]
        struct UnreadableStore {
            written: std::sync::Mutex<Vec<MemberIdleRetireOverrideRecord>>,
        }

        #[async_trait::async_trait]
        impl PersistentMetadataStore for UnreadableStore {
            async fn get_subscription_cursor(
                &self,
                _mob_id: &str,
            ) -> Result<Option<u64>, MetadataStoreError> {
                Ok(None)
            }
            async fn set_subscription_cursor(
                &self,
                _mob_id: &str,
                _cursor: u64,
            ) -> Result<(), MetadataStoreError> {
                Ok(())
            }
            async fn load_member_idle_retire_overrides(
                &self,
            ) -> Result<Vec<MemberIdleRetireOverrideRecord>, MetadataStoreError> {
                Err(MetadataStoreError::Io("disk on fire".to_string()))
            }
            async fn set_member_idle_retire_override(
                &self,
                record: &MemberIdleRetireOverrideRecord,
            ) -> Result<(), MetadataStoreError> {
                self.written
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(record.clone());
                Ok(())
            }
        }

        let early = SessionId::new();
        let overrides = ImplicitDelegateRetirementOverrides::default();
        overrides
            .insert_for_test(
                "mob-a",
                "early-fork",
                bound(DelegateIdleRetireOverride::Seconds(60), Some(&early)),
            )
            .await;
        let store = Arc::new(UnreadableStore::default());
        let restored = overrides
            .attach_durable_store(Arc::clone(&store) as Arc<dyn PersistentMetadataStore>)
            .await;
        assert_eq!(restored, 0);
        let written = store
            .written
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let written: Vec<_> = written
            .into_iter()
            .map(|record| {
                (
                    record.mob_id,
                    record.member_id,
                    record.session_id,
                    record.policy,
                )
            })
            .collect();
        assert_eq!(
            written,
            vec![(
                "mob-a".to_string(),
                "early-fork".to_string(),
                early,
                DelegateIdleRetireOverride::Seconds(60),
            )]
        );
    }

    /// Boot a primary-mob runtime whose idle sweep runs every
    /// `sweep_interval_ms` with no runtime default (only a recorded opt-in
    /// makes a primary-mob member a candidate), on the given metadata file.
    async fn boot_sweeping_runtime(
        mob_id: &str,
        state_root: &std::path::Path,
        metadata_path: &std::path::Path,
        sweep_interval_ms: u64,
    ) -> UnifiedRuntime {
        use crate::{
            DiscoverySpec, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig, SqliteMetadataStore,
        };

        let definition = meerkat_mob::MobDefinition::from_toml(&format!(
            r#"
[mob]
id = "{mob_id}"

[profiles.worker]
model = "gpt-5.5"

[profiles.worker.tools]
comms = true
"#
        ))
        .expect("mob definition");
        let mob_spec = MobBootstrapSpec::ephemeral(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            state_root.to_path_buf(),
            4,
            None,
        )
        .with_options(MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(meerkat_client::TestClient::default())),
        });
        UnifiedRuntime::bootstrap_with_options(
            mob_spec,
            MobKitConfig {
                modules: Vec::new(),
                discovery: DiscoverySpec {
                    namespace: mob_id.to_string(),
                    modules: Vec::new(),
                },
                pre_spawn: Vec::new(),
            },
            Vec::new(),
            Duration::from_secs(2),
            RuntimeOptions {
                implicit_delegate_idle_retire_secs: None,
                implicit_delegate_idle_sweep_interval_ms: sweep_interval_ms,
                ..RuntimeOptions::default()
            },
            Arc::new(SqliteMetadataStore::open(metadata_path).expect("open metadata store")),
        )
        .await
        .expect("bootstrap runtime")
    }

    async fn seat(handle: &meerkat_mob::MobHandle, member: &str) -> meerkat_core::types::SessionId {
        handle
            .ensure_member(meerkat_mob::SpawnMemberSpec::new(
                meerkat_mob::ProfileName::from("worker"),
                AgentIdentity::from(member),
            ))
            .await
            .expect("seat member");
        handle
            .resolve_bridge_session_id(&AgentIdentity::from(member))
            .await
            .expect("seated member session")
    }

    async fn is_live(handle: &meerkat_mob::MobHandle, id: &str) -> bool {
        handle
            .list_members_including_retiring()
            .await
            .iter()
            .any(|member| {
                member.agent_identity.as_str() == id && member.status != MobMemberStatus::Retiring
            })
    }

    async fn persisted_members(metadata_path: &std::path::Path) -> Vec<String> {
        use crate::{PersistentMetadataStore, SqliteMetadataStore};

        SqliteMetadataStore::open(metadata_path)
            .expect("probe metadata store")
            .load_member_idle_retire_overrides()
            .await
            .expect("load opt-ins")
            .into_iter()
            .map(|record| record.member_id)
            .collect()
    }

    /// Regression (review of #442): an opt-in whose member was retired
    /// outside the sweep must not apply to a later member seated under the
    /// same id. Two real runtimes share one metadata file: the first opts
    /// `helper` in and retires it by hand (which now releases the row at the
    /// source), the second seats a new `helper` that never opted in. A row a
    /// crash between the retire and its release would leave behind is
    /// written back; the second sweep drops it on its first visit and leaves
    /// the new member alone.
    #[tokio::test]
    async fn sweep_never_applies_a_stale_opt_in_to_a_reused_member_id() {
        const MOB_ID: &str = "reused-member-id";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");

        // First process: its sweep never runs during the test.
        let first = boot_sweeping_runtime(
            MOB_ID,
            &temp.path().join("first"),
            &metadata_path,
            3_600_000,
        )
        .await;
        let first_handle = first.mob_handle();
        let first_session = seat(&first_handle, "helper").await;
        let overrides = first
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        overrides
            .set(MOB_ID, "helper", DelegateIdleRetireOverride::Seconds(0))
            .await;
        assert_eq!(
            view(overrides.get_bound(MOB_ID, "helper").await),
            Some((
                DelegateIdleRetireOverride::Seconds(0),
                Some(first_session.clone())
            )),
            "the opt-in binds to the member's session"
        );
        assert_eq!(persisted_members(&metadata_path).await, vec!["helper"]);
        // Retired outside the sweep (the forker's mob_retire_member, an
        // operator retire): the retirement releases the opt-in row.
        first_handle
            .retire(AgentIdentity::from("helper"))
            .await
            .expect("retire helper by hand");
        tokio::time::timeout(Duration::from_secs(10), async {
            while !persisted_members(&metadata_path).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the hand retire did not release the opt-in row");
        first.shutdown().await;
        drop(first_handle);
        drop(first);
        // What a crash between the retire and the release leaves behind.
        {
            use crate::PersistentMetadataStore;
            crate::SqliteMetadataStore::open(&metadata_path)
                .expect("reopen metadata store")
                .set_member_idle_retire_override(&record(
                    MOB_ID,
                    "helper",
                    first_session.clone(),
                    DelegateIdleRetireOverride::Seconds(0),
                ))
                .await
                .expect("crash leftover row");
        }
        assert_eq!(persisted_members(&metadata_path).await, vec!["helper"]);

        // Second process: a new `helper`, seated without any opt-in.
        let second =
            boot_sweeping_runtime(MOB_ID, &temp.path().join("second"), &metadata_path, 1_000).await;
        let second_handle = second.mob_handle();
        let second_session = seat(&second_handle, "helper").await;
        assert_ne!(second_session, first_session);

        tokio::time::timeout(Duration::from_secs(10), async {
            while !persisted_members(&metadata_path).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the sweep never dropped the stale opt-in");
        assert!(
            is_live(&second_handle, "helper").await,
            "a member that never opted in must not be idle-retired by a stale opt-in"
        );
        assert_eq!(
            second
                .mob_runtime
                .implicit_delegate_retirement_overrides()
                .expect("overrides")
                .get(MOB_ID, "helper")
                .await,
            None
        );
        second.shutdown().await;
    }

    /// A restored opt-in whose member still runs the bound session re-arms
    /// the sweep: the member is retired and its opt-in cleared, a member
    /// nobody opted in stays. The restore is the attach bootstrap runs, over
    /// a row written for the seated member's session. `set` records nothing
    /// for a member that is not seated or not in a swept mob.
    #[tokio::test]
    async fn sweep_retires_a_restored_member_whose_session_still_matches() {
        use crate::{PersistentMetadataStore, SqliteMetadataStore};

        const MOB_ID: &str = "restored-member";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");
        // Nothing is a sweep candidate until the opt-in is restored below.
        let runtime =
            boot_sweeping_runtime(MOB_ID, &temp.path().join("state"), &metadata_path, 1_000).await;
        let handle = runtime.mob_handle();
        let overrides = runtime
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");

        overrides
            .set(MOB_ID, "ghost", DelegateIdleRetireOverride::Seconds(0))
            .await;
        overrides
            .set(
                "explicit-mob",
                "worker",
                DelegateIdleRetireOverride::Seconds(0),
            )
            .await;
        assert_eq!(overrides.get(MOB_ID, "ghost").await, None, "not seated");
        assert_eq!(
            overrides.get("explicit-mob", "worker").await,
            None,
            "not in a swept mob"
        );
        assert!(persisted_members(&metadata_path).await.is_empty());

        let fork_session = seat(&handle, "fork-child").await;
        seat(&handle, "bystander").await;
        SqliteMetadataStore::open(&metadata_path)
            .expect("previous process store")
            .set_member_idle_retire_override(&record(
                MOB_ID,
                "fork-child",
                fork_session,
                DelegateIdleRetireOverride::Seconds(0),
            ))
            .await
            .expect("row for the seated member");
        let restored = overrides
            .attach_durable_store(Arc::new(
                SqliteMetadataStore::open(&metadata_path).expect("reopen metadata store"),
            ))
            .await;
        assert_eq!(restored, 1);

        tokio::time::timeout(Duration::from_secs(10), async {
            while is_live(&handle, "fork-child").await {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the sweep never retired the member whose opt-in was restored");
        assert!(is_live(&handle, "bystander").await);
        tokio::time::timeout(Duration::from_secs(5), async {
            while !persisted_members(&metadata_path).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the retired member's opt-in was not cleared");

        runtime.shutdown().await;
    }

    /// A respawn's rebind carries the opt-in whichever way it races the
    /// retirement of the old session: a live opt-in moves, a just-released
    /// one is restored, a late release of the old session is a no-op, and a
    /// rebind from a session the opt-in never belonged to moves nothing.
    #[tokio::test]
    async fn rebind_carries_the_opt_in_regardless_of_retirement_order() {
        use meerkat_core::types::SessionId;

        let (old, new, unrelated) = (SessionId::new(), SessionId::new(), SessionId::new());
        let policy = DelegateIdleRetireOverride::Seconds(3600);

        // Rebind first, then the old session's retirement arrives.
        let overrides = ImplicitDelegateRetirementOverrides::default();
        overrides
            .insert_for_test("mob-a", "fork", bound(policy, Some(&old)))
            .await;
        assert!(overrides.rebind("mob-a", "fork", &old, new.clone()).await);
        overrides.release_session("mob-a", "fork", &old).await;
        assert_eq!(
            view(overrides.get_bound("mob-a", "fork").await),
            Some((policy, Some(new.clone())))
        );

        // The old session's retirement first, then the rebind.
        let overrides = ImplicitDelegateRetirementOverrides::default();
        overrides
            .insert_for_test("mob-a", "fork", bound(policy, Some(&old)))
            .await;
        overrides.release_session("mob-a", "fork", &old).await;
        assert_eq!(overrides.get_bound("mob-a", "fork").await, None);
        assert!(overrides.rebind("mob-a", "fork", &old, new.clone()).await);
        assert_eq!(
            view(overrides.get_bound("mob-a", "fork").await),
            Some((policy, Some(new.clone())))
        );

        // A rebind from a session the opt-in never belonged to (a reused id
        // respawned) moves nothing.
        let overrides = ImplicitDelegateRetirementOverrides::default();
        overrides
            .insert_for_test("mob-a", "fork", bound(policy, Some(&old)))
            .await;
        assert!(!overrides.rebind("mob-a", "fork", &unrelated, new).await);
        assert_eq!(
            view(overrides.get_bound("mob-a", "fork").await),
            Some((policy, Some(old.clone())))
        );
    }

    async fn persisted_sessions(
        metadata_path: &std::path::Path,
    ) -> Vec<(String, meerkat_core::types::SessionId)> {
        use crate::{PersistentMetadataStore, SqliteMetadataStore};

        SqliteMetadataStore::open(metadata_path)
            .expect("probe metadata store")
            .load_member_idle_retire_overrides()
            .await
            .expect("load opt-ins")
            .into_iter()
            .map(|record| (record.member_id, record.session_id))
            .collect()
    }

    /// Review of #442: a respawned member keeps its opt-in. MobKit's respawn
    /// surfaces carry it to the respawned session, in memory and durably;
    /// the old session's retirement does not take it away.
    #[tokio::test]
    async fn respawn_carries_the_opt_in_to_the_respawned_session() {
        const MOB_ID: &str = "respawn-carries-opt-in";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");
        let runtime = boot_sweeping_runtime(
            MOB_ID,
            &temp.path().join("state"),
            &metadata_path,
            3_600_000,
        )
        .await;
        let handle = runtime.mob_handle();
        let overrides = runtime
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        let before = seat(&handle, "fork-child").await;
        let policy = DelegateIdleRetireOverride::Seconds(3600);
        overrides.set(MOB_ID, "fork-child", policy).await;

        let member = AgentIdentity::from("fork-child");
        crate::mob_handle_runtime::respawn_carrying_idle_retire_opt_in(
            Some(&overrides),
            &handle,
            &member,
            handle.respawn(member.clone(), None),
            Result::is_ok,
        )
        .await
        .expect("respawn");
        let after = handle
            .resolve_bridge_session_id(&member)
            .await
            .expect("respawned session");
        assert_ne!(after, before, "a respawn mints a new session");

        // However the old session's retirement interleaved, it is a no-op
        // for the carried opt-in.
        overrides
            .release_session(MOB_ID, "fork-child", &before)
            .await;
        assert_eq!(
            view(overrides.get_bound(MOB_ID, "fork-child").await),
            Some((policy, Some(after.clone())))
        );
        tokio::time::timeout(Duration::from_secs(5), async {
            while persisted_sessions(&metadata_path).await
                != vec![("fork-child".to_string(), after.clone())]
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the durable opt-in was not rebound to the respawned session");
        runtime.shutdown().await;
    }

    /// A reset (a respawn that starts a fresh member) and a hand retire both
    /// release the opt-in: the retirement of its session, observed on the
    /// mob event stream whatever surface caused it, clears the row.
    #[tokio::test]
    async fn reset_and_hand_retire_release_the_opt_in_row() {
        const MOB_ID: &str = "retire-releases-opt-in";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");
        let runtime = boot_sweeping_runtime(
            MOB_ID,
            &temp.path().join("state"),
            &metadata_path,
            3_600_000,
        )
        .await;
        let handle = runtime.mob_handle();
        let overrides = runtime
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        for member in ["reset-me", "retire-me"] {
            seat(&handle, member).await;
            overrides
                .set(MOB_ID, member, DelegateIdleRetireOverride::Seconds(3600))
                .await;
        }
        assert_eq!(
            persisted_members(&metadata_path).await,
            vec!["reset-me", "retire-me"]
        );

        // Reset: respawned without carrying the opt-in.
        let reset = AgentIdentity::from("reset-me");
        crate::mob_handle_runtime::respawn_carrying_idle_retire_opt_in(
            None,
            &handle,
            &reset,
            handle.respawn(reset.clone(), None),
            Result::is_ok,
        )
        .await
        .expect("reset respawn");
        // Hand retire, outside the sweep.
        handle
            .retire(AgentIdentity::from("retire-me"))
            .await
            .expect("hand retire");

        tokio::time::timeout(Duration::from_secs(10), async {
            while !persisted_members(&metadata_path).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the retired sessions' opt-in rows were not cleared");
        assert_eq!(overrides.get(MOB_ID, "reset-me").await, None);
        assert_eq!(overrides.get(MOB_ID, "retire-me").await, None);
        runtime.shutdown().await;
    }

    /// An identity member's continuity rebind to a respawned (or delivery
    /// repaired) session carries its opt-in: the identity runtime tells the
    /// runtime's opt-ins which session the member moved from and to.
    #[tokio::test]
    async fn identity_continuity_rebind_carries_the_opt_in() {
        use crate::identity_first::{
            AgentAddressability, AgentIdentity as DurableIdentity, AgentRuntimeId,
            CheckpointVersion, ContinuityGeneration, ContinuityRecord, ContinuityStore,
            DurabilityPolicy, DurableAgentSpec, IdentityFirstRuntimeContext,
            IdentityLifecycleState, IdentityRuntime, IdentityRuntimeConfig, LeaseAcquireResult,
            LeaseProvider, LocalContinuityStore, LocalLeaseProvider, MobSessionBridge,
            RosterContext, RosterError, RosterProvider,
        };

        struct FixedRoster(Vec<DurableAgentSpec>);

        #[async_trait::async_trait]
        impl RosterProvider for FixedRoster {
            async fn roster(
                &self,
                _context: &RosterContext,
            ) -> Result<Vec<DurableAgentSpec>, RosterError> {
                Ok(self.0.clone())
            }
        }

        const MOB_ID: &str = "identity-rotation-carries-opt-in";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");
        let mut runtime = boot_sweeping_runtime(
            MOB_ID,
            &temp.path().join("state"),
            &metadata_path,
            3_600_000,
        )
        .await;
        let handle = runtime.mob_handle();

        let identity = DurableIdentity::parse("domain:rotating").expect("identity");
        let alias = "rt:domain:rotating:0";
        let roster_member = crate::member_comms_id::mob_member_id(alias);
        handle
            .ensure_member(meerkat_mob::SpawnMemberSpec::new(
                meerkat_mob::ProfileName::from("worker"),
                roster_member.clone(),
            ))
            .await
            .expect("seat identity member");
        let before = handle
            .resolve_bridge_session_id(&roster_member)
            .await
            .expect("member session");

        let continuity_store =
            Arc::new(LocalContinuityStore::in_memory().expect("continuity store"));
        let lease_provider = Arc::new(LocalLeaseProvider::new());
        let leases = lease_provider
            .acquire_leases(std::slice::from_ref(&identity), MOB_ID)
            .await
            .expect("identity lease");
        let lease = match leases.get(&identity) {
            Some(LeaseAcquireResult::Acquired(lease)) => lease.clone(),
            other => panic!("expected acquired lease, got {other:?}"),
        };
        let record = ContinuityRecord {
            identity: identity.clone(),
            agent_runtime_id: AgentRuntimeId::parse(alias).expect("runtime alias"),
            session_id: before.clone(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        continuity_store
            .upsert_continuity_record(&record, lease.fencing_token)
            .await
            .expect("persist continuity");
        let identity_runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store,
            lease_provider,
            runtime_instance_id: MOB_ID.to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: Some(Arc::new(MobSessionBridge::new(handle.clone()))),
            default_timeout: None,
        }));
        let spec = DurableAgentSpec {
            identity: identity.clone(),
            profile: meerkat_mob::ProfileName::from("worker"),
            addressability: AgentAddressability::Addressable,
            display_name: None,
            labels: BTreeMap::new(),
            context: None,
            additional_instructions: Vec::new(),
            initial_message: None,
            runtime_mode_override: None,
            backend: None,
            binding: None,
            placement: None,
        };
        identity_runtime
            .register(
                spec.clone(),
                IdentityLifecycleState::Active,
                Some(record),
                Some(lease),
            )
            .await;
        runtime.attach_identity_first_context(Arc::new(IdentityFirstRuntimeContext::new(
            Arc::clone(&identity_runtime),
            Arc::new(FixedRoster(vec![spec])),
            None,
            None,
            Some(runtime.mob_handle().definition().clone()),
        )));

        let overrides = runtime
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        let policy = DelegateIdleRetireOverride::Seconds(3600);
        overrides.set(MOB_ID, roster_member.as_str(), policy).await;
        assert_eq!(
            view(overrides.get_bound(MOB_ID, roster_member.as_str()).await),
            Some((policy, Some(before.clone())))
        );

        // A lower-level respawn, then the continuity rebind the control
        // surfaces and the delivery repair perform.
        handle
            .respawn(roster_member.clone(), None)
            .await
            .expect("respawn identity member");
        let after = handle
            .resolve_bridge_session_id(&roster_member)
            .await
            .expect("respawned session");
        assert_ne!(after, before);
        identity_runtime
            .rebind_session_after_live_respawn(&identity, after.clone())
            .await
            .expect("continuity rebind");

        assert_eq!(
            view(overrides.get_bound(MOB_ID, roster_member.as_str()).await),
            Some((policy, Some(after.clone())))
        );
        runtime.shutdown().await;
    }

    fn mob_event(
        kind: meerkat_mob::MobEventKind,
        at: chrono::DateTime<chrono::Utc>,
    ) -> meerkat_mob::MobEvent {
        meerkat_mob::MobEvent {
            cursor: 1,
            timestamp: at,
            mob_id: meerkat_mob::MobId::from("mob-a"),
            kind,
        }
    }

    /// Reset and destroy events release exactly the opt-ins recorded before
    /// them: an opt-in recorded after the event (the event stream lagging a
    /// quick re-seat) is kept.
    #[tokio::test]
    async fn reset_and_destroy_release_only_opt_ins_recorded_before_them() {
        use meerkat_core::types::SessionId;

        let before = chrono::Utc::now() - chrono::Duration::seconds(60);
        let after = chrono::Utc::now() + chrono::Duration::seconds(60);
        let policy = DelegateIdleRetireOverride::Seconds(3600);
        let overrides = ImplicitDelegateRetirementOverrides::default();
        let at = |at: chrono::DateTime<chrono::Utc>| {
            crate::mob_handle_runtime::BoundIdleRetireOverride {
                recorded_at: at,
                ..bound(policy, Some(&SessionId::new()))
            }
        };
        overrides
            .insert_for_test("mob-a", "old-1", at(before))
            .await;
        overrides.insert_for_test("mob-a", "new-1", at(after)).await;
        overrides
            .insert_for_test("mob-b", "old-2", at(before))
            .await;
        let now = chrono::Utc::now();

        overrides
            .observe_mob_event(
                "mob-a",
                &mob_event(meerkat_mob::MobEventKind::MobDestroying, now),
            )
            .await;
        assert_eq!(overrides.get("mob-a", "old-1").await, None);
        assert_eq!(
            overrides.get("mob-a", "new-1").await,
            Some(policy),
            "an opt-in recorded after the destroy belongs to a later member"
        );
        assert_eq!(
            overrides.get("mob-b", "old-2").await,
            Some(policy),
            "another mob is untouched"
        );

        overrides
            .insert_for_test("mob-a", "reset-me", at(before))
            .await;
        overrides
            .insert_for_test("mob-a", "reseated", at(after))
            .await;
        let reset = |member: &str| {
            mob_event(
                meerkat_mob::MobEventKind::MemberReset {
                    agent_identity: AgentIdentity::from(member),
                    previous_generation: meerkat_mob::ids::Generation::new(0),
                    new_generation: meerkat_mob::ids::Generation::new(1),
                    fence_token: meerkat_mob::ids::FenceToken::new(1),
                    agent_runtime_id: meerkat_mob::ids::AgentRuntimeId::new(
                        AgentIdentity::from(member),
                        meerkat_mob::ids::Generation::new(1),
                    ),
                },
                now,
            )
        };
        overrides
            .observe_mob_event("mob-a", &reset("reset-me"))
            .await;
        overrides
            .observe_mob_event("mob-a", &reset("reseated"))
            .await;
        assert_eq!(overrides.get("mob-a", "reset-me").await, None);
        assert_eq!(overrides.get("mob-a", "reseated").await, Some(policy));
    }

    /// A member retired and then resumed onto the exact same session (an
    /// operator re-attach, a repair's retire-then-resume) gets its opt-in
    /// back; a different session does not.
    #[tokio::test]
    async fn resuming_onto_the_same_session_restores_the_released_opt_in() {
        use meerkat_core::types::SessionId;

        let (session, other) = (SessionId::new(), SessionId::new());
        let policy = DelegateIdleRetireOverride::Seconds(3600);
        let overrides = ImplicitDelegateRetirementOverrides::default();
        overrides
            .insert_for_test("mob-a", "fork", bound(policy, Some(&session)))
            .await;
        overrides.release_session("mob-a", "fork", &session).await;
        assert_eq!(overrides.get("mob-a", "fork").await, None);

        assert_eq!(
            overrides
                .reconcile_seated("mob-a", "fork", Some(&other))
                .await,
            None,
            "a member on another session is another instance"
        );
        assert_eq!(
            overrides
                .reconcile_seated("mob-a", "fork", Some(&session))
                .await,
            Some(policy)
        );
        assert_eq!(
            view(overrides.get_bound("mob-a", "fork").await),
            Some((policy, Some(session)))
        );
    }

    /// Review of #442 (hardening): opt-ins recorded for members of implicit
    /// delegation mobs are released when their member is retired by hand or
    /// their mob is destroyed. Those mobs are not on the primary event
    /// stream; the sweep settles them.
    #[tokio::test]
    async fn implicit_mob_retire_and_destroy_release_their_opt_ins() {
        const MOB_ID: &str = "implicit-mob-release";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");
        let runtime =
            boot_sweeping_runtime(MOB_ID, &temp.path().join("state"), &metadata_path, 1_000).await;
        let state = runtime
            .mob_runtime
            .agent_mob_mcp_state()
            .expect("agent mob state");
        let overrides = runtime
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        let owner = meerkat_core::types::SessionId::new().to_string();
        let (implicit, _) = state
            .ensure_implicit_mob_for_model(&owner, "gpt-5.5", None)
            .await
            .expect("implicit mob");
        for helper in ["helper-a", "helper-b"] {
            state
                .mob_spawn(
                    &implicit,
                    meerkat_mob::ProfileName::from("delegate"),
                    AgentIdentity::from(helper),
                    Some(meerkat_mob::MobRuntimeMode::TurnDriven),
                    None,
                    None,
                )
                .await
                .expect("seat helper");
            overrides
                .set(
                    implicit.as_str(),
                    helper,
                    DelegateIdleRetireOverride::Seconds(3600),
                )
                .await;
        }
        assert_eq!(
            persisted_members(&metadata_path).await,
            vec!["helper-a", "helper-b"]
        );

        state
            .handle_for(&implicit)
            .await
            .expect("implicit handle")
            .retire(AgentIdentity::from("helper-a"))
            .await
            .expect("hand retire");
        tokio::time::timeout(Duration::from_secs(10), async {
            while persisted_members(&metadata_path).await != vec!["helper-b"] {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the hand-retired helper's opt-in was not released");

        state
            .destroy_bridge_session_mobs(&owner)
            .await
            .expect("destroy the implicit mob");
        tokio::time::timeout(Duration::from_secs(10), async {
            while !persisted_members(&metadata_path).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the destroyed implicit mob's opt-ins were not released");
        runtime.shutdown().await;
    }

    /// A member retired by hand and then resumed onto its exact session gets
    /// its opt-in back (the retirement released it; the resume restores it),
    /// as `mobkit/attach_existing_session` does on a persistent runtime.
    #[tokio::test]
    async fn retire_then_resume_onto_the_same_session_keeps_the_opt_in() {
        use crate::{DiscoverySpec, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig};

        const MOB_ID: &str = "resume-same-session";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");
        let state_root = temp.path().join("state");
        std::fs::create_dir_all(&state_root).expect("state root");
        let session_store: Arc<dyn meerkat::SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(state_root.join("sessions.sqlite3"))
                .expect("session store"),
        );
        let definition = meerkat_mob::MobDefinition::from_toml(&format!(
            "[mob]\nid = \"{MOB_ID}\"\n\n[profiles.worker]\nmodel = \"gpt-5.5\"\n\n\
             [profiles.worker.tools]\ncomms = true\n"
        ))
        .expect("mob definition");
        let spec = MobBootstrapSpec::persistent(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            state_root,
            4,
            session_store,
        )
        .expect("persistent spec")
        .with_options(MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(meerkat_client::TestClient::default())),
        });
        let runtime = UnifiedRuntime::bootstrap_with_options(
            spec,
            MobKitConfig {
                modules: Vec::new(),
                discovery: DiscoverySpec {
                    namespace: MOB_ID.to_string(),
                    modules: Vec::new(),
                },
                pre_spawn: Vec::new(),
            },
            Vec::new(),
            Duration::from_secs(2),
            RuntimeOptions {
                implicit_delegate_idle_retire_secs: None,
                implicit_delegate_idle_sweep_interval_ms: 1_000,
                ..RuntimeOptions::default()
            },
            Arc::new(crate::SqliteMetadataStore::open(&metadata_path).expect("metadata store")),
        )
        .await
        .expect("bootstrap persistent runtime");
        let handle = runtime.mob_handle();
        let overrides = runtime
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        let session = seat(&handle, "fork-child").await;
        let policy = DelegateIdleRetireOverride::Seconds(3600);
        overrides.set(MOB_ID, "fork-child", policy).await;

        let member = AgentIdentity::from("fork-child");
        handle.retire(member.clone()).await.expect("hand retire");
        tokio::time::timeout(Duration::from_secs(10), async {
            while !persisted_members(&metadata_path).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the retirement did not release the opt-in");

        Box::pin(
            handle.spawn_spec(
                meerkat_mob::SpawnMemberSpec::new(
                    meerkat_mob::ProfileName::from("worker"),
                    member.clone(),
                )
                .with_launch_mode(meerkat_mob::launch::MemberLaunchMode::Resume {
                    bridge_session_id: session.clone(),
                    resume_from_role: None,
                }),
            ),
        )
        .await
        .expect("resume onto the same session");
        assert_eq!(
            handle.resolve_bridge_session_id(&member).await,
            Some(session.clone()),
            "the member is back on its exact session"
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            while persisted_sessions(&metadata_path).await
                != vec![("fork-child".to_string(), session.clone())]
            {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the resumed member's opt-in was not restored");
        assert_eq!(overrides.get(MOB_ID, "fork-child").await, Some(policy));
        runtime.shutdown().await;
    }

    /// Review of #447: two respawns of the same member at once (a console
    /// double click, an operator and the agent's `mob_respawn` together)
    /// keep the opt-in on the member's final session, round after round.
    #[tokio::test]
    async fn concurrent_double_respawns_keep_the_opt_in() {
        const MOB_ID: &str = "concurrent-double-respawn";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");
        let runtime =
            boot_sweeping_runtime(MOB_ID, &temp.path().join("state"), &metadata_path, 1_000).await;
        let handle = runtime.mob_handle();
        let overrides = runtime
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        seat(&handle, "fork-child").await;
        let policy = DelegateIdleRetireOverride::Seconds(3600);
        overrides.set(MOB_ID, "fork-child", policy).await;
        let member = AgentIdentity::from("fork-child");

        for round in 0..10 {
            let (first, second) = tokio::join!(
                crate::mob_handle_runtime::respawn_carrying_idle_retire_opt_in(
                    Some(&overrides),
                    &handle,
                    &member,
                    handle.respawn(member.clone(), None),
                    Result::is_ok,
                ),
                crate::mob_handle_runtime::respawn_carrying_idle_retire_opt_in(
                    Some(&overrides),
                    &handle,
                    &member,
                    handle.respawn(member.clone(), None),
                    Result::is_ok,
                ),
            );
            assert!(
                first.is_ok() || second.is_ok(),
                "round {round}: at least one respawn lands"
            );
            let current = handle
                .resolve_bridge_session_id(&member)
                .await
                .expect("respawned session");
            tokio::time::timeout(Duration::from_secs(5), async {
                while view(overrides.get_bound(MOB_ID, "fork-child").await)
                    != Some((policy, Some(current.clone())))
                    || persisted_sessions(&metadata_path).await
                        != vec![("fork-child".to_string(), current.clone())]
                {
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
            .await
            .unwrap_or_else(|_| {
                panic!("round {round}: the opt-in did not stay on the respawned session")
            });
        }
        runtime.shutdown().await;
    }

    /// Review of #447: a respawning request dropped at any point (a client
    /// disconnect) may lose the respawned member's own opt-in (the
    /// documented leak-direction limit), but never leaves an opt-in that a
    /// DIFFERENT member seated under the id would pick up.
    #[tokio::test]
    async fn a_mid_respawn_request_drop_never_makes_another_member_retirable() {
        const MOB_ID: &str = "mid-respawn-drop";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");
        let runtime = boot_sweeping_runtime(
            MOB_ID,
            &temp.path().join("state"),
            &metadata_path,
            3_600_000,
        )
        .await;
        let handle = runtime.mob_handle();
        let overrides = runtime
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        let policy = DelegateIdleRetireOverride::Seconds(0);

        for (round, delay_us) in [0_u64, 100, 1_000, 3_000, 10_000, 30_000, 100_000]
            .into_iter()
            .enumerate()
        {
            let id = format!("drop-{round}");
            let member = AgentIdentity::from(id.as_str());
            seat(&handle, &id).await;
            overrides.set(MOB_ID, id.as_str(), policy).await;

            let _ = tokio::time::timeout(
                Duration::from_micros(delay_us),
                crate::mob_handle_runtime::respawn_carrying_idle_retire_opt_in(
                    Some(&overrides),
                    &handle,
                    &member,
                    handle.respawn(member.clone(), None),
                    Result::is_ok,
                ),
            )
            .await;
            // Let a respawn the drop did not cancel finish in the actor.
            let settled = tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let live = handle
                        .list_members_including_retiring()
                        .await
                        .into_iter()
                        .find(|entry| entry.agent_identity.as_str() == id)
                        .is_some_and(|entry| entry.status != MobMemberStatus::Retiring);
                    if live && let Some(session) = handle.resolve_bridge_session_id(&member).await {
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        if handle.resolve_bridge_session_id(&member).await == Some(session.clone())
                        {
                            return session;
                        }
                    }
                    tokio::time::sleep(Duration::from_millis(25)).await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("round {round}: the member never settled"));

            // Whatever survived applies to this member instance or to no one.
            if let Some(bound) = overrides.get_bound(MOB_ID, &id).await {
                let honoured = overrides
                    .reconcile_seated(MOB_ID, &id, Some(&settled))
                    .await;
                assert!(
                    honoured.is_none() || bound.session_id.as_ref() == Some(&settled),
                    "round {round}: an opt-in applied to a session it was not bound to"
                );
            }

            // A different member seated under the same id never inherits it.
            handle.retire(member.clone()).await.expect("hand retire");
            let fresh = seat(&handle, &id).await;
            assert_eq!(
                overrides.reconcile_seated(MOB_ID, &id, Some(&fresh)).await,
                None,
                "round {round}: a different member under the id became retirable"
            );
            assert_eq!(overrides.get_bound(MOB_ID, &id).await, None);
        }
        runtime.shutdown().await;
    }

    /// The implicit-mob release skips a member whose MobKit respawn is in
    /// flight: it can be missing from the roster for a moment, and its
    /// respawn carries the opt-in.
    #[tokio::test]
    async fn implicit_mob_release_skips_a_member_being_respawned() {
        const MOB_ID: &str = "implicit-respawn-skip";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");
        let runtime =
            boot_sweeping_runtime(MOB_ID, &temp.path().join("state"), &metadata_path, 1_000).await;
        let state = runtime
            .mob_runtime
            .agent_mob_mcp_state()
            .expect("agent mob state");
        let overrides = runtime
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        let owner = meerkat_core::types::SessionId::new().to_string();
        let (implicit, _) = state
            .ensure_implicit_mob_for_model(&owner, "gpt-5.5", None)
            .await
            .expect("implicit mob");
        state
            .mob_spawn(
                &implicit,
                meerkat_mob::ProfileName::from("delegate"),
                AgentIdentity::from("helper"),
                Some(meerkat_mob::MobRuntimeMode::TurnDriven),
                None,
                None,
            )
            .await
            .expect("seat helper");
        overrides
            .set(
                implicit.as_str(),
                "helper",
                DelegateIdleRetireOverride::Disabled,
            )
            .await;

        // The helper drops out of the roster while its respawn is in flight.
        let in_flight = overrides.respawn_in_flight(implicit.as_str(), "helper");
        state
            .handle_for(&implicit)
            .await
            .expect("implicit handle")
            .retire(AgentIdentity::from("helper"))
            .await
            .expect("helper leaves the roster");
        tokio::time::sleep(Duration::from_millis(2_500)).await;
        assert_eq!(
            persisted_members(&metadata_path).await,
            vec!["helper"],
            "a member being respawned keeps its opt-in through the gap"
        );

        drop(in_flight);
        tokio::time::timeout(Duration::from_secs(10), async {
            while !persisted_members(&metadata_path).await.is_empty() {
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("once no respawn is in flight, the gone helper's opt-in is released");
        runtime.shutdown().await;
    }

    /// Deterministic companion: a request dropped after its respawn committed
    /// but before its rebind leaves the opt-in bound to the gone session
    /// (here nothing releases it: this runtime has no event hook). A member
    /// seated later under the id never picks it up; the stale binding is
    /// dropped instead.
    #[tokio::test]
    async fn a_respawn_whose_rebind_never_ran_leaves_nothing_for_another_member() {
        use crate::{MobBootstrapOptions, MobBootstrapSpec};

        const MOB_ID: &str = "rebind-never-ran";
        let temp = tempfile::tempdir().expect("tempdir");
        let definition = meerkat_mob::MobDefinition::from_toml(&format!(
            "[mob]\nid = \"{MOB_ID}\"\n\n[profiles.worker]\nmodel = \"gpt-5.5\"\n\n\
             [profiles.worker.tools]\ncomms = true\n"
        ))
        .expect("mob definition");
        let runtime = crate::mob_handle_runtime::MobRuntime::bootstrap(
            MobBootstrapSpec::ephemeral(
                definition,
                meerkat_mob::MobStorage::in_memory(),
                temp.path().to_path_buf(),
                4,
                None,
            )
            .with_options(MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: Some(Arc::new(meerkat_client::TestClient::default())),
            }),
        )
        .await
        .expect("bootstrap runtime");
        let handle = runtime.handle();
        let overrides = runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        let member = AgentIdentity::from("fork-child");
        let original = seat(&handle, "fork-child").await;
        overrides
            .set(MOB_ID, "fork-child", DelegateIdleRetireOverride::Seconds(0))
            .await;

        // The respawn commits; the request that would have rebound is gone.
        handle.respawn(member.clone(), None).await.expect("respawn");
        assert_eq!(
            view(overrides.get_bound(MOB_ID, "fork-child").await),
            Some((DelegateIdleRetireOverride::Seconds(0), Some(original))),
            "the opt-in is left bound to the gone session"
        );
        handle.retire(member.clone()).await.expect("hand retire");
        let fresh = seat(&handle, "fork-child").await;
        assert_eq!(
            overrides
                .reconcile_seated(MOB_ID, "fork-child", Some(&fresh))
                .await,
            None,
            "a different member under the id must not become retirable"
        );
        assert_eq!(overrides.get_bound(MOB_ID, "fork-child").await, None);
        let _ = handle.shutdown().await;
    }

    /// Review of #447: another surface retires x and seats a new x (which
    /// never opted in) after a MobKit respawn read x's incarnation, ahead of
    /// the respawn in the actor queue. The respawn then succeeds on the new
    /// x. The member now under the id is not the successor of the one read
    /// before, so the opt-in is not carried and the new x is never
    /// idle-retired.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_retire_and_reseat_queued_ahead_of_a_respawn_never_carry_the_opt_in() {
        const MOB_ID: &str = "staged-reseat-respawn";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");
        let runtime =
            boot_sweeping_runtime(MOB_ID, &temp.path().join("state"), &metadata_path, 1_000).await;
        let handle = runtime.mob_handle();
        let overrides = runtime
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        let member = AgentIdentity::from("x");
        let original = seat(&handle, "x").await;
        overrides
            .set(MOB_ID, "x", DelegateIdleRetireOverride::Seconds(1))
            .await;

        let staged = handle.clone();
        let staged_member = member.clone();
        let respawned = crate::mob_handle_runtime::respawn_carrying_idle_retire_opt_in(
            Some(&overrides),
            &handle,
            &member,
            async move {
                // Ahead of the respawn in the actor queue: another surface
                // retires x and seats a new x.
                staged.retire(staged_member.clone()).await.expect("retire");
                seat(&staged, staged_member.as_str()).await;
                staged.respawn(staged_member, None).await
            },
            Result::is_ok,
        )
        .await;
        respawned.expect("the respawn of the new x succeeds");
        let current = handle
            .resolve_bridge_session_id(&member)
            .await
            .expect("the new x is seated");
        assert_ne!(current, original);
        assert!(
            overrides
                .get_bound(MOB_ID, "x")
                .await
                .is_none_or(|bound| bound.session_id.as_ref() != Some(&current)),
            "the new x must not hold the old x's opt-in"
        );
        assert!(
            !persisted_sessions(&metadata_path)
                .await
                .iter()
                .any(|(_, session)| session == &current),
            "no durable opt-in row for the new x"
        );
        // Several sweep passes: the new x stays.
        tokio::time::sleep(Duration::from_millis(3_500)).await;
        assert!(is_live(&handle, "x").await, "the new x was idle-retired");
        assert_eq!(
            handle.resolve_bridge_session_id(&member).await,
            Some(current)
        );
        runtime.shutdown().await;
    }

    /// A respawn that fails carries nothing, even when the member under the
    /// id changed while it ran (here another surface retired x and seated a
    /// new x first).
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_failed_respawn_never_carries_the_opt_in() {
        const MOB_ID: &str = "staged-failed-respawn";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");
        let runtime =
            boot_sweeping_runtime(MOB_ID, &temp.path().join("state"), &metadata_path, 1_000).await;
        let handle = runtime.mob_handle();
        let overrides = runtime
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        let member = AgentIdentity::from("x");
        seat(&handle, "x").await;
        overrides
            .set(MOB_ID, "x", DelegateIdleRetireOverride::Seconds(1))
            .await;

        let staged = handle.clone();
        let staged_member = member.clone();
        let outcome: Result<(), &str> =
            crate::mob_handle_runtime::respawn_carrying_idle_retire_opt_in(
                Some(&overrides),
                &handle,
                &member,
                async move {
                    staged.retire(staged_member.clone()).await.expect("retire");
                    seat(&staged, staged_member.as_str()).await;
                    Err("respawn rejected")
                },
                Result::is_ok,
            )
            .await;
        assert!(outcome.is_err());
        let fresh = handle
            .resolve_bridge_session_id(&member)
            .await
            .expect("the new x is seated");
        assert!(
            overrides
                .get_bound(MOB_ID, "x")
                .await
                .is_none_or(|bound| bound.session_id.as_ref() != Some(&fresh)),
            "the new x must not hold the old x's opt-in"
        );
        assert!(
            !persisted_sessions(&metadata_path)
                .await
                .iter()
                .any(|(_, session)| session == &fresh),
            "no durable opt-in row for the new x"
        );
        tokio::time::sleep(Duration::from_millis(3_500)).await;
        assert!(is_live(&handle, "x").await, "the new x was idle-retired");
        runtime.shutdown().await;
    }

    #[test]
    fn a_member_owns_every_live_member_below_it() {
        let [c, d, e, other] = ["c", "d", "e", "other"].map(AgentIdentity::from);
        let none = BTreeSet::new();
        let owns = |member: &AgentIdentity,
                    spawned: &[(&AgentIdentity, &AgentIdentity)],
                    retiring: &BTreeSet<AgentIdentity>| {
            spawned_subtree_has_live_member(member, spawned.iter().copied(), retiring)
        };
        assert!(!owns(&c, &[], &none), "nothing spawned");
        assert!(!owns(&c, &[(&d, &other)], &none), "another member's child");
        assert!(owns(&c, &[(&d, &c)], &none), "a live child");
        assert!(
            !owns(&d, &[(&d, &c)], &none),
            "a child does not own its spawner"
        );
        let d_retiring = BTreeSet::from([d.clone()]);
        assert!(!owns(&c, &[(&d, &c)], &d_retiring), "a retiring child");
        assert!(
            owns(&c, &[(&d, &c), (&e, &d)], &d_retiring),
            "a live grandchild below a retiring child"
        );
        let both_retiring = BTreeSet::from([d.clone(), e.clone()]);
        assert!(!owns(&c, &[(&d, &c), (&e, &d)], &both_retiring));
        assert!(
            !owns(&c, &[(&d, &e), (&e, &d)], &none),
            "a spawner cycle that does not reach c"
        );
    }

    /// An LLM whose turns hold until the gate opens: a member's turn stays
    /// running until then.
    struct GatedLlmClient {
        gate: tokio::sync::watch::Receiver<bool>,
    }

    impl meerkat_client::LlmClient for GatedLlmClient {
        fn project_replay_messages(
            &self,
            messages: &[meerkat_core::Message],
        ) -> Result<Vec<meerkat_core::Message>, meerkat_client::LlmError> {
            Ok(messages.to_vec())
        }

        fn stream<'a>(
            &'a self,
            request: &'a meerkat_client::LlmRequest,
        ) -> std::pin::Pin<
            Box<
                dyn futures::Stream<
                        Item = Result<meerkat_client::LlmEvent, meerkat_client::LlmError>,
                    > + Send
                    + 'a,
            >,
        > {
            use futures::StreamExt as _;

            let [usage, done] = crate::mob_handle_runtime::test_llm_usage::usage_then_done(
                request,
                meerkat_core::Provider::OpenAI,
                meerkat_core::types::StopReason::EndTurn,
            );
            let events = vec![
                Ok(meerkat_client::LlmEvent::TextDelta {
                    delta: "done".to_string(),
                    meta: None,
                }),
                Ok(usage),
                Ok(done),
            ];
            let mut gate = self.gate.clone();
            Box::pin(
                futures::stream::once(async move {
                    let _ = gate.wait_for(|open| *open).await;
                    futures::stream::iter(events)
                })
                .flatten(),
            )
        }

        fn provider(&self) -> meerkat_core::Provider {
            meerkat_core::Provider::OpenAI
        }

        fn health_check<'life0, 'async_trait>(
            &'life0 self,
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Result<(), meerkat_client::LlmError>>
                    + Send
                    + 'async_trait,
            >,
        >
        where
            'life0: 'async_trait,
            Self: 'async_trait,
        {
            Box::pin(async { Ok(()) })
        }
    }

    /// meerkat #1190 cascades a retire to everything the member spawned. A
    /// member C that is idle while its own fork D still runs must not be
    /// idle-retired: that would kill D with its outcome reaching nobody. Once
    /// D finishes and is idle-retired itself, C is idle-retired.
    ///
    /// D gets no opt-in while it runs: the sweep reads a candidate's
    /// execution snapshot, which waits for a running turn to end, so an
    /// opted-in running D would hold the whole pass instead.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_member_whose_fork_still_runs_is_not_idle_retired() {
        use crate::{DiscoverySpec, MobBootstrapOptions, MobBootstrapSpec, MobKitConfig};

        const MOB_ID: &str = "idle-forker";
        let temp = tempfile::tempdir().expect("tempdir");
        let metadata_path = temp.path().join("metadata.sqlite3");
        let (open_gate, gate) = tokio::sync::watch::channel(false);
        // Forks need durable transcript authority: a persistent runtime.
        let state_root = temp.path().join("state");
        std::fs::create_dir_all(&state_root).expect("state root");
        let session_store: Arc<dyn meerkat::SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(state_root.join("sessions.sqlite3"))
                .expect("session store"),
        );
        let definition = meerkat_mob::MobDefinition::from_toml(&format!(
            "[mob]\nid = \"{MOB_ID}\"\n\n[profiles.worker]\nmodel = \"gpt-5.5\"\n\n\
             [profiles.worker.tools]\ncomms = true\n"
        ))
        .expect("mob definition");
        let spec = MobBootstrapSpec::persistent(
            definition,
            meerkat_mob::MobStorage::in_memory(),
            state_root,
            4,
            session_store,
        )
        .expect("persistent spec")
        .with_options(MobBootstrapOptions {
            allow_ephemeral_sessions: true,
            notify_orchestrator_on_resume: true,
            default_llm_client: Some(Arc::new(GatedLlmClient { gate })),
        });
        let runtime = UnifiedRuntime::bootstrap_with_options(
            spec,
            MobKitConfig {
                modules: Vec::new(),
                discovery: DiscoverySpec {
                    namespace: MOB_ID.to_string(),
                    modules: Vec::new(),
                },
                pre_spawn: Vec::new(),
            },
            Vec::new(),
            Duration::from_secs(2),
            RuntimeOptions {
                implicit_delegate_idle_retire_secs: None,
                implicit_delegate_idle_sweep_interval_ms: 1_000,
                ..RuntimeOptions::default()
            },
            Arc::new(crate::SqliteMetadataStore::open(&metadata_path).expect("metadata store")),
        )
        .await
        .expect("bootstrap persistent runtime");
        let handle = runtime.mob_handle();
        let overrides = runtime
            .mob_runtime
            .implicit_delegate_retirement_overrides()
            .expect("overrides");
        let c = AgentIdentity::from("c");
        let d = AgentIdentity::from("d");
        // Turn-driven, so C runs no kickoff turn (the gate would hold it):
        // C is idle from the start.
        let mut source =
            meerkat_mob::SpawnMemberSpec::new(meerkat_mob::ProfileName::from("worker"), c.clone());
        source.runtime_mode = Some(meerkat_mob::MobRuntimeMode::TurnDriven);
        handle.ensure_member(source).await.expect("seat c");

        // C forks D from its own turn; D's turn holds until the gate opens.
        let mut fork =
            meerkat_mob::SpawnMemberSpec::new(meerkat_mob::ProfileName::from("worker"), d.clone());
        fork.runtime_mode = Some(meerkat_mob::MobRuntimeMode::TurnDriven);
        fork.initial_message = Some(meerkat_core::ContentInput::Text("long work".to_string()));
        let (_forked, run) = handle
            .fork_member_then_run_detached(
                &c,
                fork,
                None,
                "fork_result",
                256,
                meerkat_core::DurableForkSourceAdmission::CallerTurn,
                None,
                None,
            )
            .await
            .expect("c forks d");
        assert_eq!(
            handle
                .get_member(&d)
                .await
                .expect("roster read")
                .expect("d is seated")
                .spawned_by,
            Some(c.clone())
        );
        overrides
            .set(MOB_ID, "c", DelegateIdleRetireOverride::Seconds(1))
            .await;

        // Several sweep passes past C's idle window while D runs.
        tokio::time::sleep(Duration::from_millis(3_500)).await;
        assert!(is_live(&handle, "d").await, "d was retired while running");
        assert!(
            is_live(&handle, "c").await,
            "c was idle-retired while its fork still runs"
        );

        // D finishes and is idle-retired first; then C, which no longer owns
        // a live member, is idle-retired too.
        open_gate.send(true).expect("open the gate");
        tokio::time::timeout(Duration::from_secs(10), run.outcome())
            .await
            .expect("d finishes")
            .expect("d reached an outcome");
        overrides
            .set(MOB_ID, "d", DelegateIdleRetireOverride::Seconds(1))
            .await;
        let order = tokio::time::timeout(Duration::from_secs(15), async {
            let mut d_gone_while_c_live = false;
            loop {
                let (c_live, d_live) = (is_live(&handle, "c").await, is_live(&handle, "d").await);
                d_gone_while_c_live |= !d_live && c_live;
                if !c_live && !d_live {
                    return d_gone_while_c_live;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("c and d are idle-retired once d finishes");
        assert!(
            order,
            "d must go first: retiring c would have cascaded to it"
        );
        runtime.shutdown().await;
    }
}
