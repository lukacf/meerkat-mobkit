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
        let task = tokio::spawn(run_implicit_delegate_retirement(
            self.mob_runtime.clone(),
            state,
            self.mob_runtime.implicit_delegate_retirement_overrides(),
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
        for (mob_id, handle) in Box::pin(state.mob_handles_snapshot())
            .await
            .unwrap_or_default()
        {
            let is_primary_mob = mob_id.as_str() == primary_mob_id;
            let is_implicit_mob = Box::pin(state.is_implicit_mob(&mob_id)).await;
            if !is_primary_mob && !is_implicit_mob {
                continue;
            }
            for member in handle.list_members_observation_snapshot().await {
                let identity = member.agent_identity.to_string();
                let key = (mob_id.to_string(), identity.clone());
                seen.insert(key.clone());
                if member.status == MobMemberStatus::Retiring {
                    idle_since.remove(&key);
                    continue;
                }
                let per_delegate_override = match per_delegate_overrides.as_ref() {
                    Some(overrides) => overrides.get(mob_id.as_str(), &identity).await,
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
                let Some(session_id) = handle
                    .resolve_bridge_session_id(&member.agent_identity)
                    .await
                else {
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
        assert!(!idle_retirement_candidate(true, false, &no_labels, None,));
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
}
