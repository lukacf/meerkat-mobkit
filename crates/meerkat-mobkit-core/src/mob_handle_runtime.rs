use std::collections::BTreeSet;
use std::sync::Arc;

use meerkat_client::LlmClient;
use meerkat_core::AgentEvent;
use meerkat_mob::{
    MeerkatId, MemberRef, MemberState, MobBuilder, MobDefinition, MobError, MobHandle,
    MobSessionService, MobState, MobStorage, SpawnMemberSpec,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Default)]
pub struct MobBootstrapOptions {
    pub allow_ephemeral_sessions: bool,
    pub notify_orchestrator_on_resume: bool,
    pub default_llm_client: Option<Arc<dyn LlmClient>>,
}

pub struct MobBootstrapSpec {
    pub definition: MobDefinition,
    pub storage: MobStorage,
    pub session_service: Arc<dyn MobSessionService>,
    pub options: MobBootstrapOptions,
}

impl MobBootstrapSpec {
    pub fn new(
        definition: MobDefinition,
        storage: MobStorage,
        session_service: Arc<dyn MobSessionService>,
    ) -> Self {
        Self {
            definition,
            storage,
            session_service,
            options: MobBootstrapOptions {
                allow_ephemeral_sessions: true,
                notify_orchestrator_on_resume: true,
                default_llm_client: None,
            },
        }
    }

    pub fn with_options(mut self, options: MobBootstrapOptions) -> Self {
        self.options = options;
        self
    }
}

#[derive(Debug)]
pub enum MobRuntimeError {
    Mob(MobError),
    InvalidInput(&'static str),
}

impl std::fmt::Display for MobRuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mob(err) => write!(f, "{err}"),
            Self::InvalidInput(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for MobRuntimeError {}

impl From<MobError> for MobRuntimeError {
    fn from(value: MobError) -> Self {
        Self::Mob(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobMemberSnapshot {
    pub meerkat_id: String,
    pub profile: String,
    pub state: String,
    pub wired_to: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MobReconcileReport {
    pub desired: Vec<String>,
    pub retained: Vec<String>,
    pub spawned: Vec<String>,
    #[serde(default)]
    pub retired: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MobReconcileOptions {
    pub retire_stale: bool,
}

impl Default for MobReconcileOptions {
    fn default() -> Self {
        Self { retire_stale: true }
    }
}

pub struct RealInteractionSubscription {
    pub interaction_id: String,
    pub events: tokio::sync::mpsc::Receiver<AgentEvent>,
}

#[derive(Clone)]
pub struct RealMobRuntime {
    handle: MobHandle,
}

impl RealMobRuntime {
    pub async fn bootstrap(spec: MobBootstrapSpec) -> Result<Self, MobRuntimeError> {
        let mut builder = MobBuilder::new(spec.definition, spec.storage)
            .with_session_service(spec.session_service)
            .allow_ephemeral_sessions(spec.options.allow_ephemeral_sessions)
            .notify_orchestrator_on_resume(spec.options.notify_orchestrator_on_resume);

        if let Some(client) = spec.options.default_llm_client {
            builder = builder.with_default_llm_client(client);
        }

        let handle = builder.create().await?;
        Ok(Self { handle })
    }

    pub fn from_handle(handle: MobHandle) -> Self {
        Self { handle }
    }

    pub fn handle(&self) -> MobHandle {
        self.handle.clone()
    }

    pub fn status(&self) -> MobState {
        self.handle.status()
    }

    pub async fn discover(&self) -> Vec<MobMemberSnapshot> {
        self.handle
            .list_all_members()
            .await
            .into_iter()
            .map(|entry| {
                let mut wired_to = entry
                    .wired_to
                    .into_iter()
                    .map(|peer_id| peer_id.to_string())
                    .collect::<Vec<_>>();
                wired_to.sort();
                MobMemberSnapshot {
                    meerkat_id: entry.meerkat_id.to_string(),
                    profile: entry.profile.to_string(),
                    state: match entry.state {
                        MemberState::Active => "active".to_string(),
                        MemberState::Retiring => "retiring".to_string(),
                    },
                    wired_to,
                }
            })
            .collect()
    }

    pub async fn spawn(&self, spec: SpawnMemberSpec) -> Result<MemberRef, MobRuntimeError> {
        self.handle.spawn_spec(spec).await.map_err(Into::into)
    }

    /// Spawn multiple members in parallel, delegating to the underlying
    /// [`MobHandle::spawn_many`].
    ///
    /// Returns all member refs on success, or the first error encountered.
    pub async fn spawn_many(
        &self,
        specs: Vec<SpawnMemberSpec>,
    ) -> Result<Vec<MemberRef>, MobRuntimeError> {
        let results = self.handle.spawn_many(specs).await;
        results.into_iter().map(|r| r.map_err(Into::into)).collect()
    }

    pub async fn reconcile(
        &self,
        desired_specs: Vec<SpawnMemberSpec>,
    ) -> Result<MobReconcileReport, MobRuntimeError> {
        self.reconcile_with_options(desired_specs, MobReconcileOptions::default())
            .await
    }

    pub async fn reconcile_with_options(
        &self,
        desired_specs: Vec<SpawnMemberSpec>,
        options: MobReconcileOptions,
    ) -> Result<MobReconcileReport, MobRuntimeError> {
        let existing_active_members = self
            .handle
            .list_members()
            .await
            .into_iter()
            .map(|entry| entry.meerkat_id.to_string())
            .collect::<BTreeSet<_>>();
        let mut known = existing_active_members.clone();

        let mut desired = Vec::new();
        let mut retained = Vec::new();
        let mut spawned = Vec::new();
        let mut retired = Vec::new();
        let mut seen = BTreeSet::new();

        for spec in desired_specs {
            let member_id = spec.meerkat_id.to_string();
            if !seen.insert(member_id.clone()) {
                continue;
            }
            desired.push(member_id.clone());
            if known.contains(&member_id) {
                retained.push(member_id);
                continue;
            }
            self.handle.spawn_spec(spec).await?;
            known.insert(member_id.clone());
            spawned.push(member_id);
        }

        if options.retire_stale {
            let desired_set = desired.iter().cloned().collect::<BTreeSet<_>>();
            for stale_member_id in existing_active_members
                .into_iter()
                .filter(|member_id| !desired_set.contains(member_id))
            {
                self.handle
                    .retire(MeerkatId::from(stale_member_id.clone()))
                    .await?;
                retired.push(stale_member_id);
            }
        }

        Ok(MobReconcileReport {
            desired,
            retained,
            spawned,
            retired,
        })
    }

    pub async fn stop(&self) -> Result<(), MobRuntimeError> {
        self.handle.stop().await.map_err(Into::into)
    }

    pub async fn resume(&self) -> Result<(), MobRuntimeError> {
        self.handle.resume().await.map_err(Into::into)
    }

    pub async fn inject_and_subscribe(
        &self,
        member_id: &str,
        message: String,
    ) -> Result<RealInteractionSubscription, MobRuntimeError> {
        if member_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("member_id must not be empty"));
        }
        if message.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("message must not be empty"));
        }

        let subscription = self
            .handle
            .inject_and_subscribe(MeerkatId::from(member_id), message)
            .await?;

        Ok(RealInteractionSubscription {
            interaction_id: subscription.id.to_string(),
            events: subscription.events,
        })
    }
}
