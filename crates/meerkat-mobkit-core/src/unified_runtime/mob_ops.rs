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
        let member_ref = self.mob_runtime.spawn(spec).await?;
        if let Some(hook) = &self.post_spawn_hook {
            hook(vec![member_id]).await;
        }
        Ok(member_ref)
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

    /// Send a message to a mob member (fire-and-forget, no subscription).
    pub async fn send_message(
        &self,
        member_id: &str,
        message: String,
    ) -> Result<(), MobRuntimeError> {
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
        let spec = SpawnMemberSpec {
            profile_name: meerkat_mob::ProfileName::from(profile),
            meerkat_id: meerkat_mob::MeerkatId::from(meerkat_id),
            initial_message: None,
            runtime_mode: None,
            backend: None,
            context: None,
            labels: Some(labels),
            resume_session_id: None,
            additional_instructions: None,
        };
        self.ensure_member(spec).await
    }
}
