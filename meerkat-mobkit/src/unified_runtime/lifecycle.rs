//! Runtime lifecycle management — startup, shutdown, rediscovery, and periodic maintenance.

use std::future::Future;
use std::future::IntoFuture;
use std::sync::atomic::Ordering;
use std::time::Duration;

use meerkat_mob::SpawnMemberSpec;
use tokio::sync::mpsc::error::TryRecvError;

use crate::mob_handle_runtime::{
    MobRuntimeError, is_runtime_attach_readiness_refusal, runtime_attach_readiness_subject,
};
use crate::runtime::RuntimeDecisionState;

use super::types::{
    ErrorEvent, IdentityAuthorityReleaseOutcome, MobStopOutcome, RediscoverReport,
    ShutdownDrainReport, UnifiedRuntimeError, UnifiedRuntimeRunReport,
    UnifiedRuntimeShutdownReport,
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
        let stop_started = tokio::time::Instant::now();
        let mut mob_stop = self.stop_mob_quiescing().await;
        if let Err(error) = &mob_stop {
            // Report the transient attach-readiness class through the error
            // hook. `mob_stop` deliberately stays Err: the gates below (grant
            // release, terminal teardown) are conservative on a mob that did
            // not quiesce, because releasing identity authority while a member
            // is still live parks Active identities Broken. Phases 3 and 4
            // continue either way, so the shutdown is not aborted.
            let _ = self
                .report_stop_without_interrupt(error, stop_started.elapsed().as_millis() as u64);
        }

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

        // `stop` quiesces members but keeps the mob actor - and its
        // `{mob_id}/__mob_supervisor__` in-proc route - alive for resume.
        // UnifiedRuntime shutdown is terminal, and meerkat 0.8.23 refuses
        // route displacement, so a same-process cold replacement for this mob
        // id must observe the name actually freed. Drive the mob's own
        // terminal teardown (which retires the supervisor route) only AFTER
        // identity authority is released and detached: terminal member
        // teardown observed by a still-attached IdentityRuntime would park
        // Active identities Broken instead of leaving them Dormant.
        // Best-effort: a refusal leaves the route registered and is reported
        // loudly rather than failing the report.
        if mob_stop.is_ok() {
            // The mob actor deliberately retains itself as the retry owner
            // when terminal teardown catches an in-flight run
            // terminalization ("runtime teardown observed owned
            // terminalization ... after acquiring its driver authority");
            // `MobHandle::shutdown` auto-retries only the lifecycle-pending
            // error classes, so drive the remaining convergence here with a
            // bounded retry instead of abandoning the route on first refusal.
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            let mut retry_delay = Duration::from_millis(50);
            loop {
                match self.mob_handle().shutdown().await {
                    Ok(()) => break,
                    Err(error) if std::time::Instant::now() < deadline => {
                        tracing::debug!(
                            %error,
                            "mob terminal shutdown refused; retrying until the actor converges"
                        );
                        tokio::time::sleep(retry_delay).await;
                        retry_delay = (retry_delay * 2).min(Duration::from_millis(250));
                    }
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            "mob terminal shutdown after stop did not converge; the supervisor \
                             in-proc route may remain registered until process exit"
                        );
                        break;
                    }
                }
            }
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

    /// Stop the mob for teardown, degrading a transient runtime-readiness
    /// refusal instead of failing the whole stop.
    ///
    /// `MobHandle::stop` can refuse with `Runtime not ready: attached` when a
    /// member's runtime session is still mid-kickoff. That is a readiness
    /// state, not a verdict: the caller cannot make a member less attached, so
    /// a hard failure turns a millisecond-wide window into an operator-visible
    /// teardown error (the intermittent `stop` panic at test teardown). This
    /// waits the window out, and if it still has not cleared, lets teardown
    /// proceed while reporting - typed, never swallowed - that it proceeded
    /// WITHOUT interrupting. Every other refusal keeps its existing meaning.
    ///
    /// What this deliberately does NOT do: claim the member was interrupted,
    /// or return [`MobStopOutcome::Stopped`]. A mid-attach member has already
    /// had a turn admitted and its kickoff is about to bind, so "interrupted"
    /// there would be a false success the caller cannot detect at the call
    /// site - the mirror of the failure this whole item exists to fix.
    pub async fn stop_mob_for_teardown(&self) -> MobStopOutcome {
        let started = tokio::time::Instant::now();
        match self.stop_mob_quiescing().await {
            Ok(()) => MobStopOutcome::Stopped,
            Err(error) => {
                let waited_ms = started.elapsed().as_millis() as u64;
                match self.report_stop_without_interrupt(&error, waited_ms) {
                    true => MobStopOutcome::ProceededWithoutInterrupt {
                        waited_ms,
                        member: runtime_attach_readiness_subject(&error.to_string()),
                        error: error.to_string(),
                    },
                    false => MobStopOutcome::Failed(error),
                }
            }
        }
    }

    /// Classify a non-converged stop and, when it is the transient
    /// runtime-attach readiness class, say out loud that teardown proceeded
    /// without interrupting (log + typed error hook). Returns whether the
    /// refusal was that class.
    ///
    /// Both teardown entry points route their refusal through here so the
    /// condition reads identically whichever one the host used. The report
    /// states only what was observed: the stop was refused, nothing was
    /// interrupted, a turn may already be running on the named subject.
    fn report_stop_without_interrupt(&self, error: &MobRuntimeError, waited_ms: u64) -> bool {
        let error = error.to_string();
        if !is_runtime_attach_readiness_refusal(&error) {
            return false;
        }
        let member = runtime_attach_readiness_subject(&error);
        tracing::warn!(
            %error,
            waited_ms,
            member = member.as_deref().unwrap_or("<unnamed>"),
            "mob stop refused on runtime attach readiness for its whole window; teardown \
             proceeds WITHOUT an interrupt and a turn may still be running"
        );
        self.fire_error(ErrorEvent::MobStopProceededWithoutInterrupt {
            waited_ms,
            member,
            error,
        });
        true
    }

    /// Stop the mob, quiescing in-flight member work if the machine refuses.
    ///
    /// meerkat 0.7.25's mob machine rejects `Stop` while member work is in
    /// flight (`InvalidTransition { from: Running, to: Stopped }`) instead of
    /// stopping underneath it. Shutdown is an operator act on a possibly-busy
    /// mob — a gateway going down mid-turn is normal — so a busy refusal is
    /// answered by cancelling each member's in-flight work and retrying the
    /// stop over a bounded window.
    ///
    /// A `Runtime not ready: attached` refusal shares the window but not the
    /// remedy: the member is mid-kickoff rather than busy, and cancelling work
    /// it has not started cannot help, so that class is simply waited out.
    /// Any other error, or exhaustion of the window, reports the machine's
    /// last refusal untouched.
    async fn stop_mob_quiescing(&self) -> Result<(), MobRuntimeError> {
        const STOP_QUIESCE_WINDOW: Duration = Duration::from_secs(10);
        let handle = self.mob_handle();
        let deadline = tokio::time::Instant::now() + STOP_QUIESCE_WINDOW;
        let mut last = handle.stop().await;
        loop {
            let quiesce_work = match &last {
                Err(meerkat_mob::MobError::InvalidTransition { .. }) => true,
                Err(error) if is_runtime_attach_readiness_refusal(&error.to_string()) => false,
                _ => break,
            };
            if tokio::time::Instant::now() >= deadline {
                break;
            }
            if quiesce_work {
                for member in handle.list_members().await {
                    if let Ok(Some(entry)) = handle.get_member(&member.agent_identity).await {
                        // Best-effort: a member that finished between list and
                        // cancel (stale fence) is already quiesced.
                        let _ = handle
                            .cancel_all_work(entry.agent_runtime_id, entry.fence_token)
                            .await;
                    }
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod stop_degrade_tests {
    use super::*;
    use crate::unified_runtime::ErrorEvent;
    use std::sync::Arc;

    async fn empty_runtime(mob_id: &str) -> UnifiedRuntime {
        let definition = meerkat_mob::MobDefinition::from_toml(&format!(
            r#"
[mob]
id = "{mob_id}"

[profiles.worker]
model = "gpt-5.5"
"#
        ))
        .expect("definition parses");
        UnifiedRuntime::builder()
            .definition(definition)
            .default_llm_client(Arc::new(meerkat_client::TestClient::for_provider(
                meerkat_core::Provider::OpenAI,
            )))
            .build()
            .await
            .expect("runtime builds")
    }

    fn injected_refusal(detail: &str) -> MobRuntimeError {
        MobRuntimeError::Mob(meerkat_mob::MobError::Internal(detail.to_string()))
    }

    /// DELIBERATE CANARY - DO NOT convert this to `stop_mob_for_teardown`.
    ///
    /// Every mobkit teardown site that used to trip over the upstream P1
    /// (meerkat provisioner `interrupt_member` refusing with `Runtime not
    /// ready: attached` while a member's runtime session is mid-kickoff) now
    /// routes through the degrading path. That is right for those sites, whose
    /// subject is not teardown - but if we convert ALL of them, the upstream
    /// defect stops being observable to us: meerkat's fix landing looks the
    /// same as it not landing, and a future regression there becomes
    /// permanently invisible.
    ///
    /// So this one place keeps calling the RAW `MobHandle::stop()` in the
    /// window where the race lives - immediately after a spawn - and demands
    /// it succeed. When the race hits, THIS is what goes red, with a message
    /// naming the upstream defect, instead of some unrelated tool-surface test.
    ///
    /// Honest about what it is: the window cannot be forced open, so this does
    /// not reproduce the race on demand. It is a placed observation point, not
    /// a deterministic reproduction. When meerkat 0.8.24's fix lands, the
    /// refusal branch should stop occurring entirely.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn raw_mob_handle_stop_is_still_where_the_attach_readiness_defect_surfaces() {
        let runtime = empty_runtime("raw-stop-attach-readiness-canary").await;
        runtime
            .mob_handle()
            .spawn_spec(meerkat_mob::SpawnMemberSpec::from_wire(
                "worker".to_string(),
                "canary".to_string(),
                Some("You are a canary.".into()),
                None,
                None,
            ))
            .await
            .expect("canary member spawns");

        // No settling wait on purpose: that is the whole point of the site.
        if let Err(error) = runtime.mob_handle().stop().await {
            let error = error.to_string();
            assert!(
                !is_runtime_attach_readiness_refusal(&error),
                "UPSTREAM P1 OBSERVED (meerkat provisioner interrupt_member -> \
                 RuntimeNotReady while the member's runtime session is `attached`): raw \
                 MobHandle::stop refused at teardown. This canary exists to make that \
                 visible; mobkit's own teardown path degrades it via \
                 stop_mob_for_teardown. Expected to stop happening once the meerkat 0.8.24 \
                 fix lands. Refusal: {error}"
            );
            panic!("raw stop refused for an unexpected reason: {error}");
        }
    }

    /// The degrade branch, driven by an injected refusal because the live
    /// window it exists for (a member mid-kickoff) is not something a test can
    /// hold open deterministically.
    ///
    /// Two properties, both load-bearing: the transient attach-readiness class
    /// is REPORTED (typed, through the error hook) rather than swallowed, and
    /// it is reported as its own event rather than as a generic failure.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn attach_readiness_refusal_is_reported_through_the_error_hook() {
        let mut runtime = empty_runtime("stop-degrade-reported-test").await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        runtime.set_error_hook(Arc::new(move |event: ErrorEvent| {
            let tx = tx.clone();
            Box::pin(async move {
                let _ = tx.send(event);
            })
        }));

        let degraded = runtime.report_stop_without_interrupt(
            &injected_refusal(
                "runtime-backed interrupt must resolve through MeerkatMachine for \
                 019e3c52-0f1b-73d3-a5c7-4b21c2bbf131: internal error: local interrupt_member \
                 failed: Runtime not ready: attached",
            ),
            10_000,
        );
        assert!(degraded, "the attach-readiness class must degrade");

        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .expect("the degrade must reach the error hook")
            .expect("hook channel stays open");
        match event {
            ErrorEvent::MobStopProceededWithoutInterrupt {
                waited_ms,
                member,
                error,
            } => {
                assert_eq!(waited_ms, 10_000, "the reported wait must be the real one");
                assert_eq!(
                    member.as_deref(),
                    Some("019e3c52-0f1b-73d3-a5c7-4b21c2bbf131"),
                    "the report must name the subject meerkat refused on"
                );
                assert!(
                    error.contains("Runtime not ready: attached"),
                    "the refusal text must survive into the report: {error}"
                );
                // The wording is the contract here: this event must never
                // read as an interrupt or a clean stop, because a mid-attach
                // member has already had a turn admitted.
                let rendered = ErrorEvent::MobStopProceededWithoutInterrupt {
                    waited_ms,
                    member,
                    error,
                }
                .to_string();
                assert!(
                    rendered.contains("WITHOUT a successful interrupt")
                        && rendered.contains("may still be running"),
                    "the report must say teardown did not interrupt and a turn may still be \
                     running: {rendered}"
                );
                assert!(
                    !rendered.contains("interrupted") && !rendered.contains("stopped"),
                    "the report must never claim an interrupt or a clean stop: {rendered}"
                );
            }
            other => panic!("the degrade must be its own typed event, got {other:?}"),
        }

        let _ = runtime.mob_handle().stop().await;
    }

    /// A refusal outside the readiness class keeps its meaning: no degrade,
    /// no error-hook event, so a genuinely failed stop cannot be laundered
    /// into "teardown may proceed".
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn other_stop_refusals_are_not_degraded() {
        let mut runtime = empty_runtime("stop-degrade-rejects-others-test").await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        runtime.set_error_hook(Arc::new(move |event: ErrorEvent| {
            let tx = tx.clone();
            Box::pin(async move {
                let _ = tx.send(event);
            })
        }));

        assert!(
            !runtime.report_stop_without_interrupt(
                &injected_refusal("Runtime not ready: running"),
                10_000,
            ),
            "a busy runtime is not the attach-readiness class"
        );
        assert!(
            !runtime.report_stop_without_interrupt(&injected_refusal("actor task dropped"), 10_000),
            "an unrelated refusal must not degrade"
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(250), rx.recv())
                .await
                .is_err(),
            "no error-hook event may be fired for a non-degrade refusal"
        );

        let _ = runtime.mob_handle().stop().await;
    }

    /// The outcome vocabulary is what callers branch on, and the two
    /// non-failure outcomes must stay TELLABLE APART: teardown may continue
    /// after proceeding without an interrupt, but that is not a clean stop.
    /// Collapsing them is the false-success shape this item refuses.
    #[test]
    fn proceeding_without_an_interrupt_is_never_reported_as_a_clean_stop() {
        let proceeded = MobStopOutcome::ProceededWithoutInterrupt {
            waited_ms: 10_000,
            member: Some("019e3c52-0f1b-73d3-a5c7-4b21c2bbf131".to_string()),
            error: "Runtime not ready: attached".to_string(),
        };

        assert!(MobStopOutcome::Stopped.teardown_may_proceed());
        assert!(MobStopOutcome::Stopped.stopped_cleanly());

        assert!(
            proceeded.teardown_may_proceed(),
            "teardown must not be blocked by a readiness state"
        );
        assert!(
            !proceeded.stopped_cleanly(),
            "proceeding without an interrupt must never read as a clean stop"
        );

        let failed = MobStopOutcome::Failed(injected_refusal("actor task dropped"));
        assert!(!failed.teardown_may_proceed());
        assert!(!failed.stopped_cleanly());
    }

    /// The subject is extracted from what meerkat actually said, and omitted
    /// when it said nothing - never guessed.
    #[test]
    fn the_reported_subject_is_only_what_the_refusal_named() {
        assert_eq!(
            crate::mob_handle_runtime::runtime_attach_readiness_subject(
                "runtime-backed interrupt must resolve through MeerkatMachine for \
                 019e3c52-0f1b-73d3-a5c7-4b21c2bbf131: internal error: local interrupt_member \
                 failed: Runtime not ready: attached"
            )
            .as_deref(),
            Some("019e3c52-0f1b-73d3-a5c7-4b21c2bbf131")
        );
        assert_eq!(
            crate::mob_handle_runtime::runtime_attach_readiness_subject(
                "Runtime not ready: attached"
            ),
            None,
            "an unnamed refusal must report no subject rather than invent one"
        );
    }
}
