//! Mob member operations on `UnifiedRuntime`.
//!
//! Kept intentionally small: a pair of accessors (`mob_handle`, `mob_runtime`)
//! and hook-aware variants of `spawn` / `spawn_many` (which fire `post_spawn_hook`
//! and report errors via the shared error hook). All other member-lifecycle
//! operations — status, discover, reconcile, retire, helpers, etc. — are now
//! on `MobHandle` directly; callers reach through `runtime.mob_handle()`.

use meerkat_mob::{MobError, MobHandle, SpawnMemberSpec, SpawnResult};
use std::future::Future;

use crate::mob_handle_runtime::MobRuntimeError;

use super::UnifiedRuntime;

// Upstream routed runtime-ready signals use a bounded actor queue with
// fail-fast enqueue in meerkat-mob 0.6.x. Keep bulk discovery bootstrap
// serialized until the upstream signal path is backpressured.
const MAX_CONCURRENT_SPAWN_MANY: usize = 1;

/// Default ceiling for `mobkit/wait_ready` when the caller omits `timeout_ms`.
///
/// meerkat-mob 0.7.9 (#798, reactive-readiness redesign) lowered its own
/// internal `DEFAULT_READY_WAIT_TIMEOUT` from 600s to 60s; that default is
/// applied inside `MobHandle::wait_for_ready` whenever the caller passes
/// `None`. mobkit's SDK contract for `wait_ready` is "wait until the mob is
/// ready", so a caller that omits a timeout keeps the prior generous ceiling
/// rather than silently inheriting meerkat's lowered 60s — the reactive wait
/// still returns promptly when members converge, so this is only the safety
/// wall for genuinely slow startups. Pass an explicit `timeout_ms` to override.
pub(crate) const DEFAULT_WAIT_READY_TIMEOUT: std::time::Duration =
    std::time::Duration::from_mins(10);

/// Whether a `MobHandle::wait_for_ready` error is a readiness-deadline timeout
/// (which maps to the documented `{ ready: [], timeout: true }` envelope) as
/// opposed to a genuine failure (which surfaces as an RPC error).
///
/// Matches the typed variant rather than the Display string: meerkat-mob's
/// [`MobError::ReadyWaitTimedOut`] renders as `"member ready wait timed out"`,
/// which does NOT contain the substring `"timeout"` (only `"timed out"`), so
/// the previous `message.to_lowercase().contains("timeout")` check always fell
/// through to the error branch and returned `-32000` instead of the envelope.
pub(crate) fn is_ready_wait_timeout(err: &MobError) -> bool {
    matches!(err, MobError::ReadyWaitTimedOut { .. })
}

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
    ///
    /// The spec's member id is a public alias; it is encoded into the
    /// comms-safe roster id here (meerkat 0.7 `MemberCommsName` rejects `:`,
    /// which MobKit's identity-first aliases like `rt:review:singleton:0`
    /// contain). Hooks and error events keep speaking the alias.
    pub async fn spawn(&self, mut spec: SpawnMemberSpec) -> Result<SpawnResult, MobRuntimeError> {
        let member_id = spec.identity.to_string();
        let profile = spec.role_name.to_string();
        spec.identity = crate::member_comms_id::mob_member_id(member_id.as_str());
        match Box::pin(self.mob_handle().spawn_spec(spec)).await {
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
        mut specs: Vec<SpawnMemberSpec>,
    ) -> Result<Vec<SpawnResult>, MobRuntimeError> {
        let member_ids: Vec<String> = specs.iter().map(|s| s.identity.to_string()).collect();
        // As in `spawn`: wire aliases become comms-safe roster ids.
        for spec in &mut specs {
            spec.identity = crate::member_comms_id::mob_member_id(spec.identity.as_str());
        }
        let handle = self.mob_handle();
        let refs = try_join_in_batches(specs, MAX_CONCURRENT_SPAWN_MANY, |spec| {
            let handle = handle.clone();
            async move { Box::pin(handle.spawn_spec(spec)).await }
        })
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

async fn try_join_in_batches<I, F, T, E, Build>(
    items: Vec<I>,
    batch_size: usize,
    mut build: Build,
) -> Result<Vec<T>, E>
where
    F: Future<Output = Result<T, E>>,
    Build: FnMut(I) -> F,
{
    let batch_size = batch_size.max(1);
    let mut results = Vec::with_capacity(items.len());
    let mut iter = items.into_iter();

    loop {
        let batch: Vec<I> = iter.by_ref().take(batch_size).collect();
        if batch.is_empty() {
            break;
        }

        let futures = batch.into_iter().map(&mut build);
        let mut batch_results = futures::future::try_join_all(futures).await?;
        results.append(&mut batch_results);
        tokio::task::yield_now().await;
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::{is_ready_wait_timeout, try_join_in_batches};
    use meerkat_mob::MobError;

    #[tokio::test]
    async fn spawn_many_batch_size_stays_serial_until_upstream_backpressure_exists() {
        assert_eq!(super::MAX_CONCURRENT_SPAWN_MANY, 1);
    }

    #[test]
    fn ready_wait_timeout_is_classified_as_envelope_not_error() {
        // Regression (meerkat-mob 0.7.9 #798): the default ready-wait dropped
        // 600s -> 60s, so `wait_for_ready` hits this timeout far more often.
        // The prior `message.to_lowercase().contains("timeout")` never matched
        // the Display "member ready wait timed out", so timeouts wrongly
        // surfaced as a -32000 RPC error instead of `{ ready: [], timeout:true }`.
        let timed_out = MobError::ReadyWaitTimedOut {
            pending_member_ids: vec![],
        };
        assert!(is_ready_wait_timeout(&timed_out));

        // Pin the exact failure mode that motivated the typed match.
        let display = timed_out.to_string().to_lowercase();
        assert!(
            !display.contains("timeout"),
            "old substring check would have missed this timeout"
        );
        assert!(display.contains("timed out"));

        // Precision: a *kickoff* timeout also Displays "...timed out" but is not
        // a readiness timeout — it must NOT be folded into the ready envelope.
        assert!(!is_ready_wait_timeout(&MobError::KickoffWaitTimedOut {
            pending_member_ids: vec![],
        }));
    }

    #[tokio::test]
    async fn try_join_in_batches_can_run_serially() {
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let items: Vec<usize> = (0..25).collect();

        let results = try_join_in_batches(items.clone(), 1, |item| {
            let active = active.clone();
            let max_active = max_active.clone();
            async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                max_active.fetch_max(current, Ordering::SeqCst);
                tokio::task::yield_now().await;
                active.fetch_sub(1, Ordering::SeqCst);
                Ok::<_, ()>(item)
            }
        })
        .await;

        assert_eq!(results, Ok(items));
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn try_join_in_batches_limits_concurrent_work_and_preserves_order() {
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let items: Vec<usize> = (0..75).collect();

        let results = try_join_in_batches(items.clone(), 16, |item| {
            let active = active.clone();
            let max_active = max_active.clone();
            async move {
                let current = active.fetch_add(1, Ordering::SeqCst) + 1;
                max_active.fetch_max(current, Ordering::SeqCst);
                tokio::task::yield_now().await;
                active.fetch_sub(1, Ordering::SeqCst);
                Ok::<_, ()>(item)
            }
        })
        .await;

        assert_eq!(results, Ok(items));
        assert!(max_active.load(Ordering::SeqCst) <= 16);
    }

    #[tokio::test]
    async fn try_join_in_batches_stops_before_starting_later_batches_after_error() {
        let started = Arc::new(AtomicUsize::new(0));
        let items: Vec<usize> = (0..40).collect();

        let result = try_join_in_batches(items, 16, |item| {
            let started = started.clone();
            async move {
                started.fetch_add(1, Ordering::SeqCst);
                tokio::task::yield_now().await;
                if item == 20 { Err(item) } else { Ok(item) }
            }
        })
        .await;

        assert_eq!(result, Err(20));
        assert_eq!(started.load(Ordering::SeqCst), 32);
    }
}
