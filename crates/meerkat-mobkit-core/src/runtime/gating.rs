use super::module_boundary::{
    call_module_mcp_tool_json, mcp_required_error, module_uses_mcp, CORE_MODULE_MCP_TIMEOUT,
    MEMORY_CONFLICT_READ_MCP_TOOL,
};
use super::*;

impl MobkitRuntimeHandle {
    fn next_gating_sequence(&mut self) -> u64 {
        Self::next_sequence(&mut self.gating_sequence)
    }
    fn append_gating_audit(&mut self, mut entry: GatingAuditEntry) {
        let audit_sequence = self.next_gating_sequence();
        entry.audit_id = format!("gate-audit-{audit_sequence:06}");
        entry.timestamp_ms = current_time_ms();
        self.gating_audit.push(entry);
        while self.gating_audit.len() > GATING_AUDIT_MAX_RETAINED {
            self.gating_audit.remove(0);
        }
    }
    fn refresh_gating_timeouts(&mut self) {
        let now_ms = current_time_ms();
        let expired = self
            .gating_pending
            .iter()
            .filter(|(_, entry)| now_ms >= entry.deadline_at_ms)
            .map(|(pending_id, _)| pending_id.clone())
            .collect::<Vec<_>>();
        for pending_id in expired {
            if let Some(expired_entry) = self.gating_pending.remove(&pending_id) {
                self.gating_pending_order
                    .retain(|candidate| candidate != &pending_id);
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "timeout_fallback".to_string(),
                    action_id: expired_entry.action_id.clone(),
                    pending_id: Some(pending_id),
                    actor_id: expired_entry.actor_id,
                    risk_tier: expired_entry.risk_tier,
                    outcome: GatingOutcome::SafeDraft,
                    detail: serde_json::json!({
                        "fallback": "safe_draft",
                        "reason": "approval_timeout"
                    }),
                });
            }
        }
    }
    fn upsert_gating_pending_entry(&mut self, entry: GatingPendingEntry) {
        let pending_id = entry.pending_id.clone();
        self.gating_pending.insert(pending_id.clone(), entry);
        self.gating_pending_order
            .retain(|candidate| candidate != &pending_id);
        self.gating_pending_order.push(pending_id);
        while self.gating_pending_order.len() > GATING_PENDING_MAX_RETAINED {
            let oldest = self.gating_pending_order.remove(0);
            self.gating_pending.remove(&oldest);
        }
    }

    fn parse_memory_conflict_mcp_response(
        response: Value,
    ) -> Result<Option<MemoryConflictSignal>, RuntimeBoundaryError> {
        let candidate = response
            .as_object()
            .and_then(|payload| payload.get("conflict"))
            .cloned()
            .unwrap_or(response);
        if candidate.is_null() {
            return Ok(None);
        }
        serde_json::from_value::<MemoryConflictSignal>(candidate)
            .map(Some)
            .map_err(|error| {
                RuntimeBoundaryError::Mcp(McpBoundaryError::InvalidToolPayload {
                    module_id: "memory".to_string(),
                    tool: MEMORY_CONFLICT_READ_MCP_TOOL.to_string(),
                    reason: error.to_string(),
                })
            })
    }

    fn gating_memory_conflict_for_reference(
        &self,
        entity: Option<&str>,
        topic: Option<&str>,
    ) -> Result<Option<MemoryConflictSignal>, RuntimeBoundaryError> {
        if !self.is_module_loaded("memory") {
            return Ok(self.memory_conflict_for_reference(entity, topic));
        }

        let Some((memory_module, pre_spawn)) = self.module_and_prespawn("memory") else {
            return Err(mcp_required_error("memory", MEMORY_CONFLICT_READ_MCP_TOOL));
        };
        if !module_uses_mcp(memory_module, pre_spawn) {
            return Err(mcp_required_error("memory", MEMORY_CONFLICT_READ_MCP_TOOL));
        }

        let response = call_module_mcp_tool_json(
            memory_module,
            pre_spawn,
            MEMORY_CONFLICT_READ_MCP_TOOL,
            &serde_json::json!({
                "entity": entity,
                "topic": topic,
            }),
            CORE_MODULE_MCP_TIMEOUT,
        )?;
        Self::parse_memory_conflict_mcp_response(response)
    }

    pub fn evaluate_gating_action(
        &mut self,
        request: GatingEvaluateRequest,
    ) -> GatingEvaluateResult {
        self.refresh_gating_timeouts();
        let action = request.action.trim().to_string();
        let actor_id = request.actor_id.trim().to_string();
        let requested_approver = request
            .requested_approver
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let approval_recipient = request
            .approval_recipient
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let approval_channel = request
            .approval_channel
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let entity = request
            .entity
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let topic = request
            .topic
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let action_sequence = self.next_gating_sequence();
        let action_id = format!("gate-action-{action_sequence:06}");
        let risk_tier = request.risk_tier.clone();

        if matches!(request.risk_tier, GatingRiskTier::R2 | GatingRiskTier::R3) {
            if !self.memory_conflicts.is_empty() && (entity.is_none() || topic.is_none()) {
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "conflict_blocked".to_string(),
                    action_id: action_id.clone(),
                    pending_id: None,
                    actor_id: actor_id.clone(),
                    risk_tier: risk_tier.clone(),
                    outcome: GatingOutcome::SafeDraft,
                    detail: serde_json::json!({
                        "policy": "memory_conflict_context_required_v0_1",
                        "reason": "memory_conflict_context_missing",
                        "action": action.clone(),
                        "reference": {
                            "entity": entity,
                            "topic": topic,
                        },
                        "missing_context": {
                            "entity": entity.is_none(),
                            "topic": topic.is_none(),
                        },
                        "conflict_count": self.memory_conflicts.len(),
                    }),
                });
                return GatingEvaluateResult {
                    action_id,
                    action,
                    actor_id,
                    risk_tier,
                    outcome: GatingOutcome::SafeDraft,
                    pending_id: None,
                    fallback_reason: Some("memory_conflict_context_missing".to_string()),
                };
            }
            let conflict = match self
                .gating_memory_conflict_for_reference(entity.as_deref(), topic.as_deref())
            {
                Ok(conflict) => conflict,
                Err(error) => {
                    self.append_gating_audit(GatingAuditEntry {
                        audit_id: String::new(),
                        timestamp_ms: 0,
                        event_type: "memory_conflict_lookup_failed".to_string(),
                        action_id: action_id.clone(),
                        pending_id: None,
                        actor_id: actor_id.clone(),
                        risk_tier: risk_tier.clone(),
                        outcome: GatingOutcome::SafeDraft,
                        detail: serde_json::json!({
                            "policy": "memory_conflict_lookup_via_core_mcp",
                            "reason": "memory_conflict_lookup_failed",
                            "error": format!("{error:?}"),
                            "reference": {
                                "entity": entity,
                                "topic": topic,
                            },
                        }),
                    });
                    return GatingEvaluateResult {
                        action_id,
                        action,
                        actor_id,
                        risk_tier,
                        outcome: GatingOutcome::SafeDraft,
                        pending_id: None,
                        fallback_reason: Some("memory_conflict_lookup_failed".to_string()),
                    };
                }
            };
            if let Some(conflict) = conflict {
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "conflict_blocked".to_string(),
                    action_id: action_id.clone(),
                    pending_id: None,
                    actor_id: actor_id.clone(),
                    risk_tier: risk_tier.clone(),
                    outcome: GatingOutcome::SafeDraft,
                    detail: serde_json::json!({
                        "policy": "memory_conflict_block_v0_1",
                        "reason": "memory_conflict",
                        "action": action.clone(),
                        "reference": {
                            "entity": entity,
                            "topic": topic,
                        },
                        "conflict": conflict,
                    }),
                });
                return GatingEvaluateResult {
                    action_id,
                    action,
                    actor_id,
                    risk_tier,
                    outcome: GatingOutcome::SafeDraft,
                    pending_id: None,
                    fallback_reason: Some("memory_conflict".to_string()),
                };
            }
        }

        match request.risk_tier {
            GatingRiskTier::R0 | GatingRiskTier::R1 => {
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "evaluated".to_string(),
                    action_id: action_id.clone(),
                    pending_id: None,
                    actor_id: actor_id.clone(),
                    risk_tier: risk_tier.clone(),
                    outcome: GatingOutcome::Allowed,
                    detail: serde_json::json!({
                        "policy": "allow_immediate",
                        "rationale": request.rationale,
                        "action": action,
                    }),
                });
                GatingEvaluateResult {
                    action_id,
                    action,
                    actor_id,
                    risk_tier,
                    outcome: GatingOutcome::Allowed,
                    pending_id: None,
                    fallback_reason: None,
                }
            }
            GatingRiskTier::R2 => {
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "evaluated".to_string(),
                    action_id: action_id.clone(),
                    pending_id: None,
                    actor_id: actor_id.clone(),
                    risk_tier: risk_tier.clone(),
                    outcome: GatingOutcome::AllowedWithAudit,
                    detail: serde_json::json!({
                        "policy": "consequence_mode_allow_with_audit_v0_1",
                        "rationale": request.rationale,
                        "action": action,
                    }),
                });
                GatingEvaluateResult {
                    action_id,
                    action,
                    actor_id,
                    risk_tier,
                    outcome: GatingOutcome::AllowedWithAudit,
                    pending_id: None,
                    fallback_reason: None,
                }
            }
            GatingRiskTier::R3 => {
                let pending_sequence = self.next_gating_sequence();
                let pending_id = format!("gate-pending-{pending_sequence:06}");
                let created_at_ms = current_time_ms();
                let timeout_ms = request
                    .approval_timeout_ms
                    .unwrap_or(GATING_APPROVAL_TIMEOUT_DEFAULT_MS);
                let mut approval_route_id = None;
                let mut approval_delivery_id = None;
                let mut approval_notification_error = None;

                if let (Some(recipient), Some(channel)) =
                    (approval_recipient.as_ref(), approval_channel.as_ref())
                {
                    if self.is_module_loaded("router") && self.is_module_loaded("delivery") {
                        match self.resolve_routing(RoutingResolveRequest {
                            recipient: recipient.clone(),
                            channel: Some(channel.clone()),
                            retry_max: None,
                            backoff_ms: None,
                            rate_limit_per_minute: None,
                        }) {
                            Ok(resolution) => {
                                approval_route_id = Some(resolution.route_id.clone());
                                match self.send_delivery(DeliverySendRequest {
                                    resolution,
                                    payload: serde_json::json!({
                                        "kind": "gating_approval_request",
                                        "pending_id": pending_id.clone(),
                                        "action_id": action_id.clone(),
                                        "action": action.clone(),
                                        "actor_id": actor_id.clone(),
                                        "risk_tier": risk_tier.clone(),
                                        "requested_approver": requested_approver.clone(),
                                        "deadline_at_ms": created_at_ms.saturating_add(timeout_ms),
                                    }),
                                    idempotency_key: Some(format!("gating-approval-{pending_id}")),
                                }) {
                                    Ok(record) => {
                                        if record.status == "sent" {
                                            approval_delivery_id = Some(record.delivery_id);
                                        } else {
                                            approval_notification_error = Some(format!(
                                                "delivery_status:{}:{}",
                                                record.status, record.delivery_id
                                            ));
                                        }
                                    }
                                    Err(err) => {
                                        approval_notification_error =
                                            Some(format!("delivery:{err:?}"));
                                    }
                                }
                            }
                            Err(err) => {
                                approval_notification_error = Some(format!("routing:{err:?}"));
                            }
                        }
                    } else {
                        let mut missing_modules = Vec::new();
                        if !self.is_module_loaded("router") {
                            missing_modules.push("router");
                        }
                        if !self.is_module_loaded("delivery") {
                            missing_modules.push("delivery");
                        }
                        approval_notification_error = Some(format!(
                            "notification_modules_unavailable:{}",
                            missing_modules.join(",")
                        ));
                    }
                }
                let pending_entry = GatingPendingEntry {
                    pending_id: pending_id.clone(),
                    action_id: action_id.clone(),
                    action: action.clone(),
                    actor_id: actor_id.clone(),
                    risk_tier: risk_tier.clone(),
                    requested_approver,
                    approval_recipient,
                    approval_channel,
                    approval_route_id,
                    approval_delivery_id,
                    created_at_ms,
                    deadline_at_ms: created_at_ms.saturating_add(timeout_ms),
                };
                self.upsert_gating_pending_entry(pending_entry.clone());
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "pending_created".to_string(),
                    action_id: action_id.clone(),
                    pending_id: Some(pending_id.clone()),
                    actor_id: actor_id.clone(),
                    risk_tier: risk_tier.clone(),
                    outcome: GatingOutcome::PendingApproval,
                    detail: serde_json::json!({
                        "requested_approver": pending_entry.requested_approver.clone(),
                        "approval_recipient": pending_entry.approval_recipient.clone(),
                        "approval_channel": pending_entry.approval_channel.clone(),
                        "approval_route_id": pending_entry.approval_route_id.clone(),
                        "approval_delivery_id": pending_entry.approval_delivery_id.clone(),
                        "approval_notification_error": approval_notification_error,
                        "deadline_at_ms": pending_entry.deadline_at_ms,
                        "action": action,
                    }),
                });
                GatingEvaluateResult {
                    action_id,
                    action,
                    actor_id,
                    risk_tier,
                    outcome: GatingOutcome::PendingApproval,
                    pending_id: Some(pending_id),
                    fallback_reason: None,
                }
            }
        }
    }

    pub fn list_gating_pending(&mut self) -> Vec<GatingPendingEntry> {
        self.refresh_gating_timeouts();
        self.gating_pending_order
            .iter()
            .filter_map(|pending_id| self.gating_pending.get(pending_id).cloned())
            .collect()
    }

    pub fn decide_gating_action(
        &mut self,
        request: GatingDecideRequest,
    ) -> Result<GatingDecisionResult, GatingDecideError> {
        self.refresh_gating_timeouts();
        let decision = request.decision.clone();
        let reason = request.reason.clone();
        let pending_id = request.pending_id.trim().to_string();
        let approver_id = request.approver_id.trim().to_string();
        let pending_entry = self
            .gating_pending
            .remove(&pending_id)
            .ok_or_else(|| GatingDecideError::UnknownPendingId(pending_id.clone()))?;
        self.gating_pending_order
            .retain(|candidate| candidate != &pending_id);

        if matches!(decision, GatingDecision::Approve) && approver_id == pending_entry.actor_id {
            self.upsert_gating_pending_entry(pending_entry.clone());
            return Err(GatingDecideError::SelfApprovalForbidden);
        }
        if let Some(expected_approver) = pending_entry.requested_approver.as_deref() {
            if expected_approver != approver_id {
                self.upsert_gating_pending_entry(pending_entry.clone());
                return Err(GatingDecideError::ApproverMismatch {
                    expected: expected_approver.to_string(),
                    provided: approver_id,
                });
            }
        }

        let (outcome, event_type) = match decision {
            GatingDecision::Approve => (GatingOutcome::Allowed, "approval_decided"),
            GatingDecision::Reject => (GatingOutcome::SafeDraft, "rejection_decided"),
        };
        let decided_at_ms = current_time_ms();
        self.append_gating_audit(GatingAuditEntry {
            audit_id: String::new(),
            timestamp_ms: 0,
            event_type: event_type.to_string(),
            action_id: pending_entry.action_id.clone(),
            pending_id: Some(pending_id.clone()),
            actor_id: pending_entry.actor_id.clone(),
            risk_tier: pending_entry.risk_tier.clone(),
            outcome: outcome.clone(),
            detail: serde_json::json!({
                "approver_id": approver_id,
                "decision": decision.clone(),
                "reason": reason.clone(),
                "approval_route_id": pending_entry.approval_route_id.clone(),
                "approval_delivery_id": pending_entry.approval_delivery_id.clone(),
            }),
        });
        Ok(GatingDecisionResult {
            pending_id,
            action_id: pending_entry.action_id,
            approver_id,
            decision,
            outcome,
            decided_at_ms,
            reason,
        })
    }

    pub fn gating_audit_entries(&mut self, limit: usize) -> Vec<GatingAuditEntry> {
        self.refresh_gating_timeouts();
        self.gating_audit
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}
