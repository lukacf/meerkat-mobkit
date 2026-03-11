//! Mob member operations — spawn, reconcile, roster queries, and member lifecycle.

use std::collections::BTreeMap;

use meerkat_mob::{MemberRef, MobHandle, MobState, SpawnMemberSpec};

use crate::mob_handle_runtime::{MobMemberSnapshot, MobRuntimeError};

use super::UnifiedRuntime;

impl UnifiedRuntime {
    pub fn status(&self) -> MobState {
        self.mob_runtime.status()
    }

    pub fn mob_handle(&self) -> MobHandle {
        self.mob_runtime.handle()
    }

    pub async fn spawn(&self, spec: SpawnMemberSpec) -> Result<MemberRef, MobRuntimeError> {
        let member_id = spec.meerkat_id.to_string();
        let profile = spec.profile_name.to_string();
        match self.mob_runtime.spawn(spec).await {
            Ok(member_ref) => {
                if let Some(hook) = &self.post_spawn_hook {
                    hook(vec![member_id]).await;
                }
                Ok(member_ref)
            }
            Err(err) => {
                self.fire_error(super::types::ErrorEvent::SpawnFailure {
                    member_id,
                    profile,
                    error: format!("{err}"),
                });
                Err(err)
            }
        }
    }

    pub async fn spawn_many(
        &self,
        specs: Vec<SpawnMemberSpec>,
    ) -> Result<Vec<MemberRef>, MobRuntimeError> {
        let member_ids: Vec<String> = specs.iter().map(|s| s.meerkat_id.to_string()).collect();
        let refs = self.mob_runtime.spawn_many(specs).await?;
        if !member_ids.is_empty() {
            if let Some(hook) = &self.post_spawn_hook {
                hook(member_ids).await;
            }
        }
        Ok(refs)
    }

    /// Send a message to a mob member and return the accepting session ID.
    pub async fn send_message(
        &self,
        member_id: &str,
        message: String,
    ) -> Result<String, MobRuntimeError> {
        self.mob_runtime
            .send_message(member_id, message)
            .await
    }

    /// Find members matching a label key-value pair.
    pub async fn find_members(
        &self,
        label_key: &str,
        label_value: &str,
    ) -> Vec<MobMemberSnapshot> {
        self.mob_runtime.find_members(label_key, label_value).await
    }

    /// Ensure a member exists, spawning from spec if missing. Idempotent.
    pub async fn ensure_member(
        &self,
        spec: SpawnMemberSpec,
    ) -> Result<MobMemberSnapshot, MobRuntimeError> {
        self.mob_runtime.ensure_member(spec).await
    }

    pub async fn list_members(&self) -> Vec<MobMemberSnapshot> {
        self.mob_runtime.discover().await
    }

    pub async fn get_member(&self, member_id: &str) -> Option<MobMemberSnapshot> {
        self.mob_runtime.get_member(member_id).await
    }

    pub async fn retire_member(&self, member_id: &str) -> Result<(), MobRuntimeError> {
        self.mob_runtime.retire_member(member_id).await
    }

    pub async fn respawn_member(&self, member_id: &str) -> Result<(), MobRuntimeError> {
        self.mob_runtime.respawn_member(member_id).await
    }

    /// Ensure a member exists with the given labels, spawning if missing.
    ///
    /// Convenience wrapper: builds a `SpawnMemberSpec` from profile, meerkat_id,
    /// and labels, then delegates to `ensure_member`.
    pub async fn ensure_member_by_label(
        &self,
        profile: &str,
        meerkat_id: &str,
        labels: BTreeMap<String, String>,
    ) -> Result<MobMemberSnapshot, MobRuntimeError> {
        let spec = SpawnMemberSpec::new(
            meerkat_mob::ProfileName::from(profile),
            meerkat_mob::MeerkatId::from(meerkat_id),
        )
        .with_labels(labels);
        self.ensure_member(spec).await
    }
}
