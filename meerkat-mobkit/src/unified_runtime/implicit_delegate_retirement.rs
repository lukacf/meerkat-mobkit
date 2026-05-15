//! Background retirement for implicit delegation mobs.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

use meerkat_core::{AgentExecutionSnapshot, TurnPhase};
use meerkat_mob::{AgentIdentity, MemberState};
use meerkat_mob_mcp::MobMcpState;

use crate::mob_handle_runtime::MobRuntime;
use crate::runtime::RuntimeOptions;

use super::UnifiedRuntime;

impl UnifiedRuntime {
    pub(crate) async fn configure_implicit_delegate_retirement(&self, options: &RuntimeOptions) {
        let Some(idle_secs) = options.implicit_delegate_idle_retire_secs else {
            return;
        };
        let Some(state) = self.mob_runtime.agent_mob_mcp_state() else {
            return;
        };
        let sweep_interval =
            Duration::from_millis(options.implicit_delegate_idle_sweep_interval_ms.max(1_000));
        let task = tokio::spawn(run_implicit_delegate_retirement(
            self.mob_runtime.clone(),
            state,
            Duration::from_secs(idle_secs),
            sweep_interval,
        ));
        *self.implicit_delegate_retirement_task.lock().await = Some(task);
    }
}

async fn run_implicit_delegate_retirement(
    runtime: MobRuntime,
    state: Arc<MobMcpState>,
    idle_after: Duration,
    sweep_interval: Duration,
) {
    let primary_mob_id = runtime.handle().mob_id().to_string();
    let session_service = state.session_service();
    let mut idle_since: BTreeMap<(String, String), Instant> = BTreeMap::new();

    loop {
        tokio::time::sleep(sweep_interval).await;
        let mut seen = BTreeSet::new();
        for (mob_id, _mob_state) in state.mob_list().await {
            if mob_id.as_str() == primary_mob_id || !state.is_implicit_mob(&mob_id).await {
                continue;
            }
            let handle = match state.handle_for(&mob_id).await {
                Ok(handle) => handle,
                Err(error) => {
                    tracing::debug!(mob_id = %mob_id, error = %error, "implicit delegate idle sweep skipped missing mob");
                    continue;
                }
            };
            for member in handle.list_members_including_retiring().await {
                let identity = member.agent_identity.to_string();
                let key = (mob_id.to_string(), identity.clone());
                seen.insert(key.clone());
                if member.state == MemberState::Retiring {
                    idle_since.remove(&key);
                    continue;
                }
                let Some(session_id) = handle
                    .resolve_bridge_session_id(&member.agent_identity)
                    .await
                else {
                    idle_since.remove(&key);
                    continue;
                };
                let idle = match session_service.execution_snapshot(&session_id).await {
                    Ok(Some(snapshot)) => delegate_execution_is_idle(&snapshot),
                    Ok(None) => true,
                    Err(error) => {
                        tracing::debug!(
                            mob_id = %mob_id,
                            agent_identity = %identity,
                            session_id = %session_id,
                            error = %error,
                            "implicit delegate idle sweep skipped member after snapshot error"
                        );
                        false
                    }
                };
                if !idle {
                    idle_since.remove(&key);
                    continue;
                }
                let since = idle_since.entry(key.clone()).or_insert_with(Instant::now);
                if since.elapsed() < idle_after {
                    continue;
                }
                match handle.retire(AgentIdentity::from(identity.as_str())).await {
                    Ok(()) => {
                        tracing::info!(
                            mob_id = %mob_id,
                            agent_identity = %identity,
                            idle_after_ms = idle_after.as_millis() as u64,
                            "retired idle implicit delegate member"
                        );
                    }
                    Err(error) => {
                        tracing::debug!(
                            mob_id = %mob_id,
                            agent_identity = %identity,
                            error = %error,
                            "implicit delegate idle retirement failed"
                        );
                    }
                }
                idle_since.remove(&key);
            }
        }
        idle_since.retain(|key, _| seen.contains(key));
    }
}

fn delegate_execution_is_idle(snapshot: &AgentExecutionSnapshot) -> bool {
    turn_phase_is_idle(snapshot.turn_phase)
}

pub(crate) fn turn_phase_is_idle(phase: TurnPhase) -> bool {
    matches!(phase, TurnPhase::Ready) || phase.is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implicit_delegate_turn_phase_idle_classification() {
        assert!(turn_phase_is_idle(TurnPhase::Ready));
        assert!(turn_phase_is_idle(TurnPhase::Completed));
        assert!(turn_phase_is_idle(TurnPhase::Failed));
        assert!(turn_phase_is_idle(TurnPhase::Cancelled));

        assert!(!turn_phase_is_idle(TurnPhase::ApplyingPrimitive));
        assert!(!turn_phase_is_idle(TurnPhase::CallingLlm));
        assert!(!turn_phase_is_idle(TurnPhase::WaitingForOps));
        assert!(!turn_phase_is_idle(TurnPhase::DrainingBoundary));
        assert!(!turn_phase_is_idle(TurnPhase::Extracting));
        assert!(!turn_phase_is_idle(TurnPhase::ErrorRecovery));
        assert!(!turn_phase_is_idle(TurnPhase::Cancelling));
    }
}
