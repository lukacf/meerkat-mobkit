//! Runtime lifecycle management — startup, shutdown, rediscovery, and periodic maintenance.

use std::future::Future;
use std::sync::atomic::Ordering;
use std::time::Duration;

use meerkat_mob::SpawnMemberSpec;
use serde_json::json;
use tokio::runtime::RuntimeFlavor;
use tokio::sync::mpsc::error::TryRecvError;

use crate::mob_handle_runtime::MobRuntimeError;
use crate::runtime::{
    MobkitRuntimeHandle, RuntimeDecisionState, ScheduleDefinition, ScheduleDispatchReport,
    ScheduleValidationError,
};
use crate::types::{EventEnvelope, ModuleEvent, UnifiedEvent};

use super::types::{
    RediscoverReport, ShutdownDrainReport, UnifiedRuntimeError, UnifiedRuntimeRunReport,
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
        let spawned: Vec<String> = spawn_specs
            .iter()
            .map(|s| s.meerkat_id.to_string())
            .collect();

        // 3. Spawn discovered members
        self.mob_runtime.spawn_many(spawn_specs).await?;
        if let Some(hook) = &self.post_spawn_hook {
            hook(spawned.clone()).await;
        }

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
        let serve_result = axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_signal)
            .await;
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
        axum::serve(listener, app).await
    }

    pub async fn shutdown(&self) -> UnifiedRuntimeShutdownReport {
        self.shutting_down.store(true, Ordering::SeqCst);

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

        // Phase 2: Close event router
        self.close_event_router().await;

        // Phase 3: Shutdown modules and mob
        let module_shutdown = self.module_runtime.lock().await.shutdown();
        let mob_stop = self.mob_runtime.stop().await;
        UnifiedRuntimeShutdownReport {
            drain,
            module_shutdown,
            mob_stop,
        }
    }

    pub(super) async fn drain_mob_agent_events(&self) -> Result<(), UnifiedRuntimeError> {
        let mut disconnected = false;
        let mut ingress_guard = self
            .mob_event_ingress
            .try_lock()
            .map_err(|_| UnifiedRuntimeError::RuntimeShuttingDown)?;
        let ingress = match ingress_guard.as_mut() {
            Some(i) => i,
            None => return Ok(()),
        };

        loop {
            match Self::try_recv_ingress_event(ingress) {
                Some(Ok(unified_event)) => {
                    // Detect agent run failures and fire HostLoopCrash
                    if let crate::types::UnifiedEvent::Agent {
                        ref agent_id,
                        ref event_type,
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
            Some(MobEventIngress::Pull(router)) => {
                router.cancel();
            }
            Some(MobEventIngress::Forwarder(forwarder)) => {
                let task = forwarder.task;
                task.abort();
                let _ = task.await;
            }
            None => {}
        }
    }

    fn try_recv_ingress_event(
        ingress: &mut MobEventIngress,
    ) -> Option<Result<EventEnvelope<UnifiedEvent>, TryRecvError>> {
        Some(match ingress {
            MobEventIngress::Pull(router) => router
                .event_rx
                .try_recv()
                .map(super::attributed_event_to_unified),
            MobEventIngress::Forwarder(forwarder) => forwarder.event_rx.try_recv(),
        })
    }

    pub async fn dispatch_schedule_tick(
        &self,
        schedules: &[ScheduleDefinition],
        tick_ms: u64,
    ) -> Result<ScheduleDispatchReport, UnifiedRuntimeError> {
        if self.shutting_down.load(Ordering::SeqCst) {
            return Err(UnifiedRuntimeError::RuntimeShuttingDown);
        }
        let mut dispatch_report = self
            .dispatch_schedule_tick_blocking(schedules, tick_ms)
            .await?;

        for dispatch in &mut dispatch_report.dispatched {
            let Some(runtime_injection) = dispatch.runtime_injection.clone() else {
                continue;
            };

            let injection_result = self
                .mob_runtime
                .send_message(
                    &runtime_injection.member_id,
                    runtime_injection.message.clone(),
                )
                .await;

            match injection_result {
                Ok(session_id) => {
                    self.module_runtime
                        .lock()
                        .await
                        .append_normalized_event(EventEnvelope {
                            event_id: format!("{}-executed", runtime_injection.injection_event_id),
                            source: "module".to_string(),
                            timestamp_ms: dispatch.tick_ms,
                            event: UnifiedEvent::Module(ModuleEvent {
                                module: "runtime".to_string(),
                                event_type: "runtime.injection.executed".to_string(),
                                payload: json!({
                                    "schedule_id": dispatch.schedule_id.clone(),
                                    "claim_key": dispatch.claim_key.clone(),
                                    "member_id": runtime_injection.member_id,
                                    "message": runtime_injection.message,
                                    "session_id": session_id,
                                }),
                            }),
                        })?;
                }
                Err(error) => {
                    dispatch.runtime_injection_error =
                        Some(format!("mob injection failed: {error}"));
                    self.module_runtime
                        .lock()
                        .await
                        .append_normalized_event(EventEnvelope {
                            event_id: format!("{}-failed", runtime_injection.injection_event_id),
                            source: "module".to_string(),
                            timestamp_ms: dispatch.tick_ms,
                            event: UnifiedEvent::Module(ModuleEvent {
                                module: "runtime".to_string(),
                                event_type: "runtime.injection.failed".to_string(),
                                payload: json!({
                                    "schedule_id": dispatch.schedule_id.clone(),
                                    "claim_key": dispatch.claim_key.clone(),
                                    "member_id": runtime_injection.member_id,
                                    "message": runtime_injection.message,
                                    "error_kind": "mob_runtime",
                                    "error": format!("mob injection failed: {error}"),
                                }),
                            }),
                        })?;
                }
            }
        }

        self.drain_mob_agent_events().await?;
        Ok(dispatch_report)
    }

    async fn dispatch_schedule_tick_blocking(
        &self,
        schedules: &[ScheduleDefinition],
        tick_ms: u64,
    ) -> Result<ScheduleDispatchReport, UnifiedRuntimeError> {
        let mut rt = self.module_runtime.lock().await;

        let dispatch_result = if tokio::runtime::Handle::try_current()
            .is_ok_and(|handle| handle.runtime_flavor() == RuntimeFlavor::MultiThread)
        {
            tokio::task::block_in_place(|| {
                Self::dispatch_schedule_tick_in_joined_thread(&mut rt, schedules, tick_ms)
            })
        } else {
            Self::dispatch_schedule_tick_in_joined_thread(&mut rt, schedules, tick_ms)
        };

        dispatch_result
            .map_err(|_| UnifiedRuntimeError::ScheduleDispatchThreadPanicked)?
            .map_err(UnifiedRuntimeError::ScheduleValidation)
    }

    fn dispatch_schedule_tick_in_joined_thread(
        module_runtime: &mut MobkitRuntimeHandle,
        schedules: &[ScheduleDefinition],
        tick_ms: u64,
    ) -> std::thread::Result<Result<ScheduleDispatchReport, ScheduleValidationError>> {
        std::thread::scope(|scope| {
            scope
                .spawn(move || module_runtime.dispatch_schedule_tick(schedules, tick_ms))
                .join()
        })
    }
}
