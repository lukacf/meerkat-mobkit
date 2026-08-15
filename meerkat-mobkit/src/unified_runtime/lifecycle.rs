//! Runtime lifecycle management — startup, shutdown, rediscovery, and periodic maintenance.

use std::future::Future;
use std::future::IntoFuture;
use std::sync::atomic::Ordering;
use std::time::Duration;

use meerkat_mob::SpawnMemberSpec;
use tokio::sync::mpsc::error::TryRecvError;

use crate::mob_handle_runtime::MobRuntimeError;
use crate::runtime::RuntimeDecisionState;

use super::types::{
    IdentityAuthorityReleaseOutcome, RediscoverReport, ShutdownDrainReport, UnifiedRuntimeError,
    UnifiedRuntimeRunReport, UnifiedRuntimeShutdownReport,
};
use super::{MobEventIngress, UnifiedRuntime, discovery_spec_to_spawn_spec};

impl UnifiedRuntime {
    /// Reset the mob and re-run discovery + edge reconciliation.
    ///
    /// Sequence:
    /// 1. `MobHandle::reset()` — retires all members, clears projections,
    ///    restarts MCP servers, returns mob to Running state
    /// 2. Re-runs the stored `Discovery` (with `Value::Null` context since
    ///    `PreSpawnHook` is consumed at boot and cannot be replayed)
    /// 3. Spawns discovered members via `spawn_many`
    /// 4. Clears managed dynamic edges (stale after reset)
    /// 5. Runs edge reconciliation if `EdgeDiscovery` is configured
    ///
    /// Returns `None` if no `Discovery` is configured (nothing to rediscover).
    pub async fn rediscover(&self) -> Result<Option<RediscoverReport>, MobRuntimeError> {
        match self.rediscover_inner().await {
            Ok(report) => Ok(report),
            Err(err) => {
                self.fire_error(super::types::ErrorEvent::RediscoverFailure {
                    error: format!("{err}"),
                });
                Err(err)
            }
        }
    }

    async fn rediscover_inner(&self) -> Result<Option<RediscoverReport>, MobRuntimeError> {
        let discovery = match &self.discovery {
            Some(d) => d,
            None => return Ok(None),
        };
        if self.identity_runtime().is_some() {
            return Err(MobRuntimeError::InvalidConfig(
                "rediscover resets the whole mob and is unavailable with identity-first authority; use refresh_desired_topology"
                    .to_string(),
            ));
        }

        // 1. Reset the mob — retires all, clears state, returns to Running
        self.mob_runtime
            .handle()
            .reset()
            .await
            .map_err(MobRuntimeError::Mob)?;

        // 2. Re-run discovery (no pre-spawn context — PreSpawnHook is FnOnce)
        let specs = discovery.discover(serde_json::Value::Null).await;
        let spawn_specs: Vec<SpawnMemberSpec> =
            specs.iter().map(discovery_spec_to_spawn_spec).collect();
        let spawned: Vec<String> = spawn_specs.iter().map(|s| s.identity.to_string()).collect();

        // 3. Spawn discovered members (hook-aware variant fires post_spawn_hook)
        self.spawn_many(spawn_specs).await?;

        // 4. Clear stale managed edges (old topology is gone after reset)
        self.managed_dynamic_edges.write().await.clear();

        // 5. Reconcile edges
        let edges = self.reconcile_edges().await;

        Ok(Some(RediscoverReport { spawned, edges }))
    }

    pub async fn run<F>(
        &self,
        listener: tokio::net::TcpListener,
        decisions: RuntimeDecisionState,
        shutdown_signal: F,
    ) -> UnifiedRuntimeRunReport
    where
        F: Future<Output = ()> + Send + 'static,
    {
        let app = self.build_reference_app_router(decisions);
        let serve = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal)
            .into_future();
        tokio::pin!(serve);
        let serve_result = loop {
            tokio::select! {
                result = &mut serve => break result,
                () = tokio::time::sleep(Duration::from_millis(25)) => {
                    let _ = self.drain_mob_agent_events().await;
                }
            }
        };
        let shutdown = self.shutdown().await;
        UnifiedRuntimeRunReport {
            serve_result,
            shutdown,
        }
    }

    pub async fn serve(
        &self,
        listener: tokio::net::TcpListener,
        decisions: RuntimeDecisionState,
    ) -> std::io::Result<()> {
        let app = self.build_reference_app_router(decisions);
        let serve = axum::serve(listener, app).into_future();
        tokio::pin!(serve);
        loop {
            tokio::select! {
                result = &mut serve => break result,
                () = tokio::time::sleep(Duration::from_millis(25)) => {
                    let _ = self.drain_mob_agent_events().await;
                }
            }
        }
    }

    /// Spawn a detached task that periodically drains mob agent events and
    /// projects them onto the ConsoleEventStore. Returns a [`JoinHandle`] —
    /// callers that manage graceful shutdown should abort it before stopping
    /// the runtime.
    ///
    /// Use this when embedding [`UnifiedRuntime`] inside a host-owned axum
    /// server (so [`Self::serve`]'s built-in drain loop isn't running).
    /// Without this task the mob event router fills up, agent turns never
    /// reach the console SSE stream, and event-log consumers miss events.
    pub fn spawn_event_drain_task(self: std::sync::Arc<Self>) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(25)).await;
                if self.shutting_down.load(Ordering::SeqCst) {
                    break;
                }
                if let Err(err) = self.drain_mob_agent_events().await {
                    if matches!(err, UnifiedRuntimeError::RuntimeShuttingDown) {
                        break;
                    }
                    // Transient drain failures are logged but don't stop the
                    // task — the next tick will try again.
                    tracing::warn!(error = %err, "mob agent event drain tick failed");
                }
            }
        })
    }

    pub async fn shutdown(&self) -> UnifiedRuntimeShutdownReport {
        self.shutting_down.store(true, Ordering::SeqCst);
        // The liveness probe is observation only. Abort it before anything
        // can quiesce the mob actor so an intentional shutdown stall cannot
        // page a false ActorLoopStalled.
        if let Some(task) = self.actor_loop_probe_task.lock().await.take() {
            task.abort();
            let _ = task.await;
        }
        // The fact tail owns only a lossy wake projection, so shutdown does
        // not drain it. Abort and join the sole runtime-owned task before
        // taking down the authoritative WorkGraph-bearing mob runtime.
        if let Some(task) = self.workgraph_fact_tail_task.lock().await.take() {
            task.abort();
            let _ = task.await;
        }
        if let Some(observer) = self.agent_memory_observer_task.lock().await.take() {
            observer.abort_and_join().await;
        }
        if let Some(task) = self.agent_memory_steward_task.lock().await.take() {
            task.abort();
            let _ = task.await;
        }
        // Reconnect probes are observation accelerants only, but they own
        // sockets and may be mid-authentication. Stop and join the sole task
        // before closing the listener or quiescing member/session authority.
        if let Some(task) = self.remote_host_reconnect_task.lock().await.take() {
            abort_and_join_remote_host_reconnect(task).await;
        }
        // Stop accepting cross-mob control RPC before the mob quiesces so a
        // late inbound wire/inject cannot race member teardown.
        if let Some(task) = self.cross_mob_control_task.lock().await.take() {
            task.abort();
            let _ = task.await;
        }
        let identity_runtime = self.identity_runtime().cloned();
        if let Some(identity_runtime) = identity_runtime.as_ref() {
            // Close request admission before any supervisor is drained. A
            // caller may disappear while its lazy materialization owns an
            // uninstalled lease; the runtime, not the caller, owns that task
            // through its explicit commit/rollback boundary.
            identity_runtime.close_foreground_operations();
        }
        // A continuity repair pass owns the same serialized bootstrap
        // controller as explicit reconcile. Cancel it while idle, or join an
        // in-flight pass to its explicit lease/bridge commit boundary, before
        // waiting for background hydration.
        if let Some(task) = self.identity_continuity_repair_task.lock().await.take() {
            task.cancel_and_join().await;
        }
        if let Some(identity_runtime) = identity_runtime.as_ref() {
            // Background hydration owns concrete member creation/resume work;
            // stop and join it before quiescing the mob actor.
            identity_runtime.cancel_identity_bootstrap().await;
            // Foreground request tasks can share the same materialization and
            // lifecycle locks. Join them after the warmer has stopped and
            // before lease renewal or the mob actor is taken down.
            identity_runtime.join_foreground_operations().await;
        }
        if let Some(task) = self.implicit_delegate_retirement_task.lock().await.take() {
            task.abort();
            let _ = task.await;
        }

        // Reset commits the replacement continuity generation before the old
        // physical member can finish its archive protocol. Those exact
        // post-commit obligations live in a dedicated runtime-owned task set
        // and debt ledger: join them after foreground lifecycle admission is
        // closed, then synchronously retry every remaining pair before Mob
        // stop can observe a stale Retiring anchor.
        let mut reset_bridge_cleanup_error = None;
        if let Some(identity_runtime) = identity_runtime.as_ref() {
            identity_runtime.join_reset_bridge_cleanup_tasks().await;
            if let Err(error) = identity_runtime.drain_pending_reset_bridge_cleanups().await {
                tracing::warn!(
                    %error,
                    "reset-superseded bridge cleanup remains before mob shutdown"
                );
                reset_bridge_cleanup_error = Some(error.to_string());
            }
        }

        // Phase 1: Drain in-flight events
        let drain_start = std::time::Instant::now();
        let mut drained_count = 0_usize;
        let drain_result = tokio::time::timeout(self.drain_timeout, async {
            loop {
                if self.drain_mob_agent_events().await.is_err() {
                    break;
                }
                let ingress = self.mob_event_ingress.lock().await;
                if ingress.is_none() {
                    break;
                }
                drop(ingress);
                drained_count += 1;
                tokio::time::sleep(Duration::from_millis(50)).await;
                if drained_count > 1 {
                    break;
                }
            }
        })
        .await;
        let drain = ShutdownDrainReport {
            drained_count,
            timed_out: drain_result.is_err(),
            drain_duration_ms: drain_start.elapsed().as_millis() as u64,
        };

        // Phase 2: Stop the mob actor while its router/module dependencies
        // are still alive. Closing them first can race Stop against an
        // already-dropped actor reply channel under teardown pressure.
        let mut mob_stop = self.stop_mob_quiescing().await;

        // A first cleanup attempt can fail while the Mob stop itself finishes
        // quiescing the old runtime. Retry the retained exact debt once more;
        // if it converges after a failed stop, retry Stop so cleanup attestation
        // reflects the final structural state rather than the first refusal.
        if reset_bridge_cleanup_error.is_some()
            && let Some(identity_runtime) = identity_runtime.as_ref()
        {
            match identity_runtime.drain_pending_reset_bridge_cleanups().await {
                Ok(_) => {
                    reset_bridge_cleanup_error = None;
                    if mob_stop.is_err() {
                        mob_stop = self.stop_mob_quiescing().await;
                    }
                }
                Err(error) => {
                    tracing::warn!(
                        %error,
                        "reset-superseded bridge cleanup remains after mob shutdown retry"
                    );
                    reset_bridge_cleanup_error = Some(error.to_string());
                }
            }
        }

        // `stop` quiesces members but keeps the mob actor - and its
        // `{mob_id}/__mob_supervisor__` in-proc route - alive for resume.
        // UnifiedRuntime shutdown is terminal, and meerkat 0.8.23 refuses
        // route displacement, so a same-process cold replacement for this mob
        // id must observe the name actually freed. Drive the mob's own
        // terminal teardown (which retires the supervisor route) now that the
        // members are quiesced. Best-effort: a refusal leaves the route
        // registered and is reported loudly rather than failing the report.
        if mob_stop.is_ok()
            && let Err(error) = self.mob_handle().shutdown().await
        {
            tracing::warn!(
                %error,
                "mob terminal shutdown after stop did not converge; the supervisor \
                 in-proc route may remain registered until process exit"
            );
        }

        // Fencing authority must outlive the physical members it protects.
        // Keep renewal running through mob quiescence, then stop it before the
        // final provider release so no renewal can race the release boundary.
        if let Some(task) = self.identity_lease_renewal_task.lock().await.take() {
            task.cancel_and_join().await;
        }
        let identity_authority_release = match identity_runtime.as_ref() {
            None => IdentityAuthorityReleaseOutcome::NotConfigured,
            Some(identity_runtime) if mob_stop.is_ok() && reset_bridge_cleanup_error.is_none() => {
                match identity_runtime.release_all_leases_for_shutdown().await {
                    Ok(grant_count) => IdentityAuthorityReleaseOutcome::Released { grant_count },
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            "failed to release identity authority after mob shutdown"
                        );
                        IdentityAuthorityReleaseOutcome::Failed {
                            error: error.to_string(),
                        }
                    }
                }
            }
            Some(_) if mob_stop.is_ok() => {
                let error = reset_bridge_cleanup_error.unwrap_or_else(|| {
                    "reset bridge cleanup remained without an error detail".to_string()
                });
                tracing::warn!(
                    %error,
                    "retaining identity grants because reset bridge cleanup did not converge"
                );
                IdentityAuthorityReleaseOutcome::SkippedResetCleanupFailed { error }
            }
            Some(_) => {
                tracing::warn!(
                    "mob shutdown did not quiesce physical members; retaining identity grants"
                );
                IdentityAuthorityReleaseOutcome::SkippedMobStopFailed
            }
        };
        if mob_stop.is_ok() {
            // Break the MobRuntime <-> IdentityRuntime authority cycle only
            // after physical members are gone. This is required for failed
            // builders to release persistent topology/store locks before
            // returning Err; on a failed mob stop the authority and grants
            // deliberately remain intact.
            self.mob_runtime.clear_identity_runtime_authority();
            *self
                .implicit_delegate_identity_runtime
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }

        // Phase 3: Close event router
        self.close_event_router().await;

        // Phase 4: Shutdown modules
        let module_shutdown = self.module_runtime.lock().await.shutdown();
        UnifiedRuntimeShutdownReport {
            drain,
            module_shutdown,
            mob_stop,
            identity_authority_release,
        }
    }

    /// Stop the mob, quiescing in-flight member work if the machine refuses.
    ///
    /// meerkat 0.7.25's mob machine rejects `Stop` while member work is in
    /// flight (`InvalidTransition { from: Running, to: Stopped }`) instead of
    /// stopping underneath it. Shutdown is an operator act on a possibly-busy
    /// mob — a gateway going down mid-turn is normal — so a busy refusal is
    /// answered by cancelling each member's in-flight work and retrying the
    /// stop over a bounded window. Any other error, or exhaustion of the
    /// window, reports the machine's last refusal untouched.
    async fn stop_mob_quiescing(&self) -> Result<(), MobRuntimeError> {
        const STOP_QUIESCE_WINDOW: Duration = Duration::from_secs(10);
        let handle = self.mob_handle();
        let deadline = tokio::time::Instant::now() + STOP_QUIESCE_WINDOW;
        let mut last = handle.stop().await;
        while let Err(meerkat_mob::MobError::InvalidTransition { .. }) = &last {
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            for member in handle.list_members().await {
                if let Ok(Some(entry)) = handle.get_member(&member.agent_identity).await {
                    // Best-effort: a member that finished between list and
                    // cancel (stale fence) is already quiesced.
                    let _ = handle
                        .cancel_all_work(entry.agent_runtime_id, entry.fence_token)
                        .await;
                }
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            last = handle.stop().await;
        }
        last.map_err(MobRuntimeError::from)
    }

    /// Drain pending agent/module events from the mob event router and
    /// project them onto the ConsoleEventStore + event log. Callers that
    /// embed `UnifiedRuntime` inside their own axum server (rather than
    /// using `.serve()`) must poll this periodically — typically via
    /// [`UnifiedRuntime::spawn_event_drain_task`] — or console/event-log
    /// consumers will never see agent responses.
    pub async fn drain_mob_agent_events(&self) -> Result<(), UnifiedRuntimeError> {
        let mut disconnected = false;
        let mut ingress_guard = match self.mob_event_ingress.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                // A previous drain tick may still be projecting a burst of
                // events. Skip this tick instead of killing the host-owned
                // background drain task.
                return Ok(());
            }
        };
        let ingress = match ingress_guard.as_mut() {
            Some(i) => i,
            None => return Ok(()),
        };

        loop {
            match Self::try_recv_ingress_event(ingress) {
                Some(Ok(forwarded)) => {
                    // Fire the typed alert the forwarder extracted at ingest
                    // (e.g. a member compaction persistence rejection). The
                    // hook is read here, at drain time, so hooks installed
                    // via `set_error_hook` after construction still fire.
                    if let Some(alert) = forwarded.alert {
                        self.fire_error(alert);
                    }
                    let unified_event = forwarded.envelope;
                    // Detect agent run failures and fire HostLoopCrash
                    if let crate::types::UnifiedEvent::Agent {
                        ref agent_id,
                        ref event_type,
                        ..
                    } = unified_event.event
                        && event_type == "run_failed"
                    {
                        self.fire_error(super::types::ErrorEvent::HostLoopCrash {
                            member_id: agent_id.clone(),
                            error: format!(
                                "agent run failed (event_id: {})",
                                unified_event.event_id
                            ),
                        });
                    }
                    // Ingest into event log (non-blocking, buffered)
                    self.ingest_event(&unified_event);
                    self.project_console_event_from_unified(&unified_event)
                        .await;
                    self.module_runtime
                        .lock()
                        .await
                        .append_normalized_event(unified_event)?;
                }
                Some(Err(TryRecvError::Empty)) => break,
                Some(Err(TryRecvError::Disconnected)) => {
                    disconnected = true;
                    break;
                }
                None => break,
            }
        }

        if disconnected {
            *ingress_guard = None;
        }

        Ok(())
    }

    pub(super) async fn close_event_router(&self) {
        let ingress = self.mob_event_ingress.lock().await.take();
        match ingress {
            Some(MobEventIngress::Forwarder(forwarder)) => {
                let task = forwarder.task;
                task.abort();
                let _ = task.await;
                let health_task = forwarder.identity_stream_health_task;
                health_task.abort();
                let _ = health_task.await;
            }
            None => {}
        }

        // Stop the structural mob-events subscription task as well.
        if let Some(task) = self.mob_events_subscriber_task.lock().await.take() {
            task.abort();
            let _ = task.await;
        }
    }

    fn try_recv_ingress_event(
        ingress: &mut MobEventIngress,
    ) -> Option<Result<super::ForwardedMemberEvent, TryRecvError>> {
        Some(match ingress {
            MobEventIngress::Forwarder(forwarder) => forwarder.event_rx.try_recv(),
        })
    }
}

async fn abort_and_join_remote_host_reconnect(task: tokio::task::JoinHandle<()>) {
    task.abort();
    let _ = task.await;
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod remote_host_task_tests {
    use super::abort_and_join_remote_host_reconnect;

    struct DropNotify(Option<tokio::sync::oneshot::Sender<()>>);

    impl Drop for DropNotify {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[tokio::test]
    async fn reconnect_task_abort_is_joined_before_shutdown_continues() {
        let (sender, receiver) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _notify = DropNotify(Some(sender));
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;

        abort_and_join_remote_host_reconnect(task).await;
        receiver
            .await
            .expect("joined task must drop all owned reconnect state");
    }
}
