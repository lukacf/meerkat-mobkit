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
                let bound_override = match per_delegate_overrides.as_ref() {
                    Some(overrides) => overrides.get_bound(mob_id.as_str(), &identity).await,
                    None => None,
                };
                // An opt-in holds only for the member instance it was set
                // for. A member id can be reused after its member is retired
                // (outside this sweep, or before a restart); an opt-in bound
                // to another session belongs to that gone member and is
                // dropped instead of applying to this one.
                let mut member_session = None;
                let per_delegate_override = match bound_override.as_ref() {
                    None => None,
                    Some(bound) => match bound.session_id.as_ref() {
                        None => Some(bound.policy),
                        Some(bound_session) => {
                            member_session = handle
                                .resolve_bridge_session_id(&member.agent_identity)
                                .await;
                            match member_session.as_ref() {
                                Some(current) if current == bound_session => Some(bound.policy),
                                Some(_) => {
                                    if let Some(overrides) = per_delegate_overrides.as_ref() {
                                        overrides.clear(mob_id.as_str(), &identity, bound).await;
                                    }
                                    None
                                }
                                // Not resolvable right now (for instance
                                // before a resumed mob activates): keep the
                                // opt-in and decide on a later pass.
                                None => None,
                            }
                        }
                    },
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
                let session_id = match member_session {
                    Some(session_id) => Some(session_id),
                    None => {
                        handle
                            .resolve_bridge_session_id(&member.agent_identity)
                            .await
                    }
                };
                let Some(session_id) = session_id else {
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
                        if let (Some(overrides), Some(bound)) =
                            (per_delegate_overrides.as_ref(), bound_override.as_ref())
                        {
                            overrides.clear(mob_id.as_str(), &identity, bound).await;
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
        }
    }

    /// The attach restores the bound opt-ins a previous process recorded and
    /// persists the bound opt-ins set before it (unbound ones, which only
    /// standalone wiring produces, are never persisted).
    #[tokio::test]
    async fn attach_restores_bound_opt_ins_and_persists_earlier_ones() {
        use crate::{MemberIdleRetireOverrideRecord, PersistentMetadataStore, SqliteMetadataStore};
        use meerkat_core::types::SessionId;

        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("metadata.sqlite3");
        let (previous, early) = (SessionId::new(), SessionId::new());
        SqliteMetadataStore::open(&path)
            .expect("open metadata store")
            .set_member_idle_retire_override(&MemberIdleRetireOverrideRecord {
                mob_id: "mob-a".to_string(),
                member_id: "fork-child".to_string(),
                session_id: previous.clone(),
                policy: DelegateIdleRetireOverride::Seconds(300),
            })
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
            overrides.get_bound("mob-a", "fork-child").await,
            Some(bound(
                DelegateIdleRetireOverride::Seconds(300),
                Some(&previous)
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
        assert_eq!(
            written,
            vec![MemberIdleRetireOverrideRecord {
                mob_id: "mob-a".to_string(),
                member_id: "early-fork".to_string(),
                session_id: early,
                policy: DelegateIdleRetireOverride::Seconds(60),
            }]
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
            overrides.get_bound(MOB_ID, "helper").await,
            Some(bound(
                DelegateIdleRetireOverride::Seconds(0),
                Some(&first_session)
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
            use crate::{MemberIdleRetireOverrideRecord, PersistentMetadataStore};
            crate::SqliteMetadataStore::open(&metadata_path)
                .expect("reopen metadata store")
                .set_member_idle_retire_override(&MemberIdleRetireOverrideRecord {
                    mob_id: MOB_ID.to_string(),
                    member_id: "helper".to_string(),
                    session_id: first_session.clone(),
                    policy: DelegateIdleRetireOverride::Seconds(0),
                })
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
        use crate::{MemberIdleRetireOverrideRecord, PersistentMetadataStore, SqliteMetadataStore};

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
            .set_member_idle_retire_override(&MemberIdleRetireOverrideRecord {
                mob_id: MOB_ID.to_string(),
                member_id: "fork-child".to_string(),
                session_id: fork_session,
                policy: DelegateIdleRetireOverride::Seconds(0),
            })
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
            overrides.get_bound("mob-a", "fork").await,
            Some(bound(policy, Some(&new)))
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
            overrides.get_bound("mob-a", "fork").await,
            Some(bound(policy, Some(&new)))
        );

        // A rebind from a session the opt-in never belonged to (a reused id
        // respawned) moves nothing.
        let overrides = ImplicitDelegateRetirementOverrides::default();
        overrides
            .insert_for_test("mob-a", "fork", bound(policy, Some(&old)))
            .await;
        assert!(!overrides.rebind("mob-a", "fork", &unrelated, new).await);
        assert_eq!(
            overrides.get_bound("mob-a", "fork").await,
            Some(bound(policy, Some(&old)))
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
            overrides.get_bound(MOB_ID, "fork-child").await,
            Some(bound(policy, Some(&after)))
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
            overrides.get_bound(MOB_ID, roster_member.as_str()).await,
            Some(bound(policy, Some(&before)))
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
            overrides.get_bound(MOB_ID, roster_member.as_str()).await,
            Some(bound(policy, Some(&after)))
        );
        runtime.shutdown().await;
    }
}
