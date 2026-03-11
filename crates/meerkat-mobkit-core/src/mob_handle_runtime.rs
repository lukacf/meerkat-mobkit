use std::collections::BTreeSet;
use std::sync::Arc;

use meerkat_client::LlmClient;
use meerkat_mob::{
    MeerkatId, MemberRef, MemberState, MobBuilder, MobDefinition, MobError, MobHandle,
    MobSessionService, MobState, MobStorage, RosterEntry, SpawnMemberSpec,
};
use serde::{Deserialize, Serialize};

pub const MEMBER_STATE_ACTIVE: &str = "active";
pub const MEMBER_STATE_RETIRING: &str = "retiring";

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
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub labels: std::collections::BTreeMap<String, String>,
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

fn snapshot_from_entry(entry: RosterEntry) -> MobMemberSnapshot {
    let mut wired_to: Vec<String> = entry.wired_to.into_iter().map(|p| p.to_string()).collect();
    wired_to.sort();
    MobMemberSnapshot {
        meerkat_id: entry.meerkat_id.to_string(),
        profile: entry.profile.to_string(),
        state: match entry.state {
            MemberState::Active => MEMBER_STATE_ACTIVE.to_string(),
            MemberState::Retiring => MEMBER_STATE_RETIRING.to_string(),
        },
        wired_to,
        labels: entry.labels,
    }
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
            .map(snapshot_from_entry)
            .collect()
    }

    pub async fn get_member(&self, member_id: &str) -> Option<MobMemberSnapshot> {
        self.handle
            .get_member(&MeerkatId::from(member_id))
            .await
            .map(snapshot_from_entry)
    }

    pub async fn retire_member(&self, member_id: &str) -> Result<(), MobRuntimeError> {
        if member_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("member_id must not be empty"));
        }
        self.handle
            .retire(MeerkatId::from(member_id))
            .await
            .map_err(Into::into)
    }

    pub async fn respawn_member(&self, member_id: &str) -> Result<(), MobRuntimeError> {
        if member_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("member_id must not be empty"));
        }
        self.handle
            .respawn(MeerkatId::from(member_id), None)
            .await
            .map_err(Into::into)
    }

    pub async fn spawn(&self, spec: SpawnMemberSpec) -> Result<MemberRef, MobRuntimeError> {
        self.handle.spawn_spec(spec).await.map_err(Into::into)
    }

    pub async fn spawn_many(
        &self,
        specs: Vec<SpawnMemberSpec>,
    ) -> Result<Vec<MemberRef>, MobRuntimeError> {
        let futs = specs
            .into_iter()
            .map(|spec| self.handle.spawn_spec(spec));
        futures::future::try_join_all(futs)
            .await
            .map_err(Into::into)
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

    /// Send a message to a member (enforces external_addressable).
    pub async fn send_message(
        &self,
        member_id: &str,
        message: String,
    ) -> Result<(), MobRuntimeError> {
        if member_id.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("member_id must not be empty"));
        }
        if message.trim().is_empty() {
            return Err(MobRuntimeError::InvalidInput("message must not be empty"));
        }
        self.handle
            .send_message(MeerkatId::from(member_id), message)
            .await
            .map(|_session_id| ())
            .map_err(Into::into)
    }

    /// Find members matching a label key-value pair.
    pub async fn find_members(
        &self,
        label_key: &str,
        label_value: &str,
    ) -> Vec<MobMemberSnapshot> {
        self.discover()
            .await
            .into_iter()
            .filter(|m| m.labels.get(label_key).is_some_and(|v| v == label_value))
            .collect()
    }

    /// Ensure a member exists, spawning from spec if missing.
    ///
    /// Idempotent — returns Ok if the member already exists.
    pub async fn ensure_member(
        &self,
        spec: SpawnMemberSpec,
    ) -> Result<MobMemberSnapshot, MobRuntimeError> {
        let meerkat_id = spec.meerkat_id.clone();
        // Check roster first
        if let Some(entry) = self.handle.get_member(&meerkat_id).await {
            return Ok(snapshot_from_entry(entry));
        }
        // Spawn
        match self.handle.spawn_spec(spec).await {
            Ok(_member_ref) => {}
            Err(MobError::MeerkatAlreadyExists(_)) => {
                // Concurrent spawn — fine
            }
            Err(err) => return Err(err.into()),
        }
        // Return current state
        let entry = self
            .handle
            .get_member(&meerkat_id)
            .await
            .ok_or(MobRuntimeError::Mob(MobError::MeerkatNotFound(meerkat_id)))?;
        Ok(snapshot_from_entry(entry))
    }
}
