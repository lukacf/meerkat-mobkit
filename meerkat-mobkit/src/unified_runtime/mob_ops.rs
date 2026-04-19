//! Mob member operations — spawn, reconcile, roster queries, and member lifecycle.

use std::collections::BTreeMap;

use meerkat_core::service::SessionHistoryPage;
use meerkat_mob::launch::ForkContext;
use meerkat_mob::{
    HelperOptions, HelperResult, MobHandle, MobMemberSnapshot as RichMobMemberSnapshot, MobRun,
    MobState, SpawnMemberSpec, SpawnResult,
};

use crate::mob_handle_runtime::{MobMemberSnapshot, MobRuntimeError, MobkitMemberSessionRef};

use super::UnifiedRuntime;

impl UnifiedRuntime {
    pub async fn status(&self) -> Result<MobState, MobRuntimeError> {
        self.mob_runtime.status().await
    }

    pub fn mob_handle(&self) -> MobHandle {
        self.mob_runtime.handle()
    }

    /// Access the underlying `MobRuntime`.
    pub fn mob_runtime(&self) -> &crate::mob_handle_runtime::MobRuntime {
        &self.mob_runtime
    }

    pub async fn spawn(&self, spec: SpawnMemberSpec) -> Result<SpawnResult, MobRuntimeError> {
        let member_id = spec.identity.to_string();
        let profile = spec.role_name.to_string();
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
    ) -> Result<Vec<SpawnResult>, MobRuntimeError> {
        let member_ids: Vec<String> = specs.iter().map(|s| s.identity.to_string()).collect();
        let refs = self.mob_runtime.spawn_many(specs).await?;
        if !member_ids.is_empty()
            && let Some(hook) = &self.post_spawn_hook
        {
            hook(member_ids).await;
        }
        Ok(refs)
    }

    /// Send a message to a mob member and return the accepting session ID.
    pub async fn send_message(
        &self,
        member_id: &str,
        content: impl Into<meerkat_core::ContentInput>,
    ) -> Result<String, MobRuntimeError> {
        self.mob_runtime.send_message(member_id, content).await
    }

    /// Find members matching a label key-value pair.
    pub async fn find_members(&self, label_key: &str, label_value: &str) -> Vec<MobMemberSnapshot> {
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
            meerkat_mob::ids::MeerkatId::from(meerkat_id),
        )
        .with_labels(labels);
        self.ensure_member(spec).await
    }

    // -----------------------------------------------------------------------
    // 0.5 API surface
    // -----------------------------------------------------------------------

    /// Detailed execution snapshot for a single member.
    pub async fn member_status(
        &self,
        member_id: &str,
    ) -> Result<RichMobMemberSnapshot, MobRuntimeError> {
        self.mob_runtime.member_status(member_id).await
    }

    /// Forcefully cancel a member.
    pub async fn force_cancel_member(&self, member_id: &str) -> Result<(), MobRuntimeError> {
        self.mob_runtime.force_cancel_member(member_id).await
    }

    /// Spawn a short-lived helper member, wait for it, and return the result.
    pub async fn spawn_helper(
        &self,
        meerkat_id: &str,
        task: &str,
        options: HelperOptions,
    ) -> Result<HelperResult, MobRuntimeError> {
        self.mob_runtime
            .spawn_helper(meerkat_id, task, options)
            .await
    }

    /// Fork from an existing member's context, wait for completion, and return.
    pub async fn fork_helper(
        &self,
        source_member_id: &str,
        meerkat_id: &str,
        task: &str,
        fork_context: ForkContext,
        options: HelperOptions,
    ) -> Result<HelperResult, MobRuntimeError> {
        self.mob_runtime
            .fork_helper(source_member_id, meerkat_id, task, fork_context, options)
            .await
    }

    /// Attach a member to an existing session (resume mode).
    pub async fn attach_existing_session(
        &self,
        profile: &str,
        meerkat_id: &str,
        session_id_str: &str,
    ) -> Result<RichMobMemberSnapshot, MobRuntimeError> {
        self.mob_runtime
            .attach_existing_session(profile, meerkat_id, session_id_str)
            .await
    }

    /// Cancel a running flow by its run ID.
    pub async fn cancel_flow(&self, run_id_str: &str) -> Result<(), MobRuntimeError> {
        self.mob_runtime.cancel_flow(run_id_str).await
    }

    /// Query the status of a flow run.
    pub async fn flow_status(&self, run_id_str: &str) -> Result<Option<MobRun>, MobRuntimeError> {
        self.mob_runtime.flow_status(run_id_str).await
    }

    /// Collect all members that have reached a terminal state.
    pub async fn collect_completed(&self) -> Vec<(String, RichMobMemberSnapshot)> {
        self.mob_runtime.collect_completed().await
    }

    /// Get the current session ID for a member (if any).
    pub async fn member_current_session_id(
        &self,
        member_id: &str,
    ) -> Result<Option<String>, MobRuntimeError> {
        self.mob_runtime.member_current_session_id(member_id).await
    }

    pub async fn read_session_history(
        &self,
        session_id: &str,
        offset: usize,
        limit: Option<usize>,
    ) -> Result<SessionHistoryPage, MobRuntimeError> {
        self.mob_runtime
            .read_session_history(session_id, offset, limit)
            .await
    }

    /// Get a reference to a member's current session bridge.
    pub async fn member_session_ref(
        &self,
        member_id: &str,
    ) -> Result<Option<MobkitMemberSessionRef>, MobRuntimeError> {
        self.mob_runtime.member_session_ref(member_id).await
    }
}
