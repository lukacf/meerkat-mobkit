//! Mob member operations on `UnifiedRuntime`.
//!
//! Kept intentionally small: a pair of accessors (`mob_handle`, `mob_runtime`)
//! and hook-aware variants of `spawn` / `spawn_many` (which fire `post_spawn_hook`
//! and report errors via the shared error hook). All other member-lifecycle
//! operations — status, discover, reconcile, retire, helpers, etc. — are now
//! on `MobHandle` directly; callers reach through `runtime.mob_handle()`.

use meerkat_mob::{MobHandle, SpawnMemberSpec, SpawnResult};

use crate::mob_handle_runtime::MobRuntimeError;

use super::UnifiedRuntime;

impl UnifiedRuntime {
    pub fn mob_handle(&self) -> MobHandle {
        self.mob_runtime.handle()
    }

    /// Access the underlying `MobRuntime` (owns the session service + ephemeral dir).
    pub fn mob_runtime(&self) -> &crate::mob_handle_runtime::MobRuntime {
        &self.mob_runtime
    }

    /// Spawn a member, firing `post_spawn_hook` on success and the shared error
    /// hook on failure. For raw spawning without hooks, use `mob_handle().spawn_spec(...)`.
    pub async fn spawn(&self, spec: SpawnMemberSpec) -> Result<SpawnResult, MobRuntimeError> {
        let member_id = spec.identity.to_string();
        let profile = spec.role_name.to_string();
        match self.mob_handle().spawn_spec(spec).await {
            Ok(result) => {
                if let Some(hook) = &self.post_spawn_hook {
                    hook(vec![member_id]).await;
                }
                Ok(result)
            }
            Err(err) => {
                self.fire_error(super::types::ErrorEvent::SpawnFailure {
                    member_id,
                    profile,
                    error: format!("{err}"),
                });
                Err(err.into())
            }
        }
    }

    /// Spawn many members, firing `post_spawn_hook` once on success with all ids.
    pub async fn spawn_many(
        &self,
        specs: Vec<SpawnMemberSpec>,
    ) -> Result<Vec<SpawnResult>, MobRuntimeError> {
        let member_ids: Vec<String> = specs.iter().map(|s| s.identity.to_string()).collect();
        let handle = self.mob_handle();
        let futs = specs.into_iter().map(|spec| {
            let handle = handle.clone();
            async move { handle.spawn_spec(spec).await }
        });
        let refs = futures::future::try_join_all(futs)
            .await
            .map_err(MobRuntimeError::from)?;
        if !member_ids.is_empty()
            && let Some(hook) = &self.post_spawn_hook
        {
            hook(member_ids).await;
        }
        Ok(refs)
    }
}
