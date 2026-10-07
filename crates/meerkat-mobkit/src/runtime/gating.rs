//! Gating subsystem — policy evaluation, audit logging, and module-backed decisions.

use super::module_boundary::{
    CORE_MODULE_MCP_TIMEOUT, MEMORY_CONFLICT_READ_MCP_TOOL, call_module_mcp_tool_json,
    mcp_required_error, module_uses_mcp,
};
use super::*;

fn valid_gating_origin(origin: &GatingOrigin) -> bool {
    !origin.identity.trim().is_empty()
        && origin
            .conversation_id
            .as_deref()
            .is_none_or(|value| !value.trim().is_empty())
        && origin
            .interaction_id
            .as_deref()
            .is_none_or(|value| !value.trim().is_empty())
}

fn valid_gating_epoch(epoch: &str) -> bool {
    epoch.len() == 32
        && epoch
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// A gate identity in one of its two canonical forms.
enum GatingIdForm<'a> {
    /// `gate-<kind>-<sequence>`, minted before owner epochs.
    Legacy(u64),
    /// `gate-<kind>-v2-<epoch>-<sequence>`.
    Epoch { epoch: &'a str, sequence: u64 },
}

fn parse_gating_id<'a>(value: &'a str, prefix: &str) -> Option<GatingIdForm<'a>> {
    let canonical = |digits: &str| {
        digits
            .parse::<u64>()
            .ok()
            .filter(|sequence| digits == format!("{sequence:06}"))
    };
    let rest = value.strip_prefix(prefix)?;
    match rest
        .strip_prefix(GATING_ID_FORMAT)
        .and_then(|rest| rest.strip_prefix('-'))
    {
        Some(rest) => {
            let (epoch, digits) = rest.split_once('-')?;
            if !valid_gating_epoch(epoch) {
                return None;
            }
            Some(GatingIdForm::Epoch {
                epoch,
                sequence: canonical(digits)?,
            })
        }
        None => canonical(rest).map(GatingIdForm::Legacy),
    }
}

/// Whether `pending_id` is a pre-epoch sequential pending ID
/// (`gate-pending-<sequence>`). Every restart of an unpersisted owner
/// reissued those IDs, so one does not identify a single decision.
pub(crate) fn is_legacy_sequential_pending_id(pending_id: &str) -> bool {
    matches!(
        parse_gating_id(pending_id, "gate-pending-"),
        Some(GatingIdForm::Legacy(_))
    )
}

fn validate_gating_snapshot(snapshot: &GatingStateSnapshot) -> Result<(), GatingStateRestoreError> {
    use GatingStateRestoreError::InvalidSnapshot;
    let legacy = match snapshot.version {
        1 => true,
        2 => false,
        version => return Err(GatingStateRestoreError::UnsupportedVersion(version)),
    };
    match snapshot.owner_epoch.as_deref() {
        Some(_) if legacy => {
            return Err(InvalidSnapshot("version 1 snapshot names an owner epoch"));
        }
        Some(epoch) if !valid_gating_epoch(epoch) => {
            return Err(InvalidSnapshot("invalid owner epoch"));
        }
        _ => {}
    }
    if snapshot.pending.len() > GATING_PENDING_MAX_RETAINED
        || snapshot.audit.len() > GATING_AUDIT_MAX_RETAINED
    {
        return Err(InvalidSnapshot("retention bounds exceeded"));
    }
    if legacy
        && snapshot
            .next_sequence
            .checked_add(snapshot.pending.len() as u64 + 1)
            .is_none()
    {
        return Err(InvalidSnapshot("sequence is exhausted"));
    }
    // A version 1 snapshot holds only its own sequential IDs, all below its
    // frontier. A version 2 snapshot may also hold restored legacy IDs and
    // IDs of earlier owners; only the exporting owner's IDs have a frontier.
    let valid_id = |value: &str, prefix: &str| match parse_gating_id(value, prefix) {
        Some(GatingIdForm::Legacy(sequence)) => !legacy || sequence < snapshot.next_sequence,
        Some(GatingIdForm::Epoch { epoch, sequence }) => {
            !legacy
                && (snapshot.owner_epoch.as_deref() != Some(epoch)
                    || sequence < snapshot.next_sequence)
        }
        None => false,
    };
    let mut pending_ids = BTreeSet::new();
    for entry in &snapshot.pending {
        if !valid_id(&entry.pending_id, "gate-pending-")
            || !valid_id(&entry.action_id, "gate-action-")
            || !pending_ids.insert(entry.pending_id.as_str())
        {
            return Err(InvalidSnapshot("invalid or duplicate pending ID"));
        }
        if !matches!(entry.risk_tier, GatingRiskTier::R3)
            || entry.deadline_at_ms < entry.created_at_ms
        {
            return Err(InvalidSnapshot("invalid pending policy or deadline"));
        }
        if entry
            .origin
            .as_ref()
            .is_some_and(|origin| !valid_gating_origin(origin))
        {
            return Err(InvalidSnapshot("invalid pending origin"));
        }
    }
    let mut audit_ids = BTreeSet::new();
    for entry in &snapshot.audit {
        if !valid_id(&entry.audit_id, "gate-audit-")
            || !valid_id(&entry.action_id, "gate-action-")
            || !audit_ids.insert(entry.audit_id.as_str())
            || entry
                .pending_id
                .as_deref()
                .is_some_and(|id| !valid_id(id, "gate-pending-"))
        {
            return Err(InvalidSnapshot("invalid or duplicate audit ID"));
        }
        if entry
            .pending_id
            .as_deref()
            .is_some_and(|id| pending_ids.contains(id))
            && matches!(
                entry.event_type.as_str(),
                "approval_decided"
                    | "rejection_decided"
                    | "escalation_decided"
                    | "timeout_fallback"
            )
        {
            return Err(InvalidSnapshot("resolved request is also pending"));
        }
        if let Some(value) = entry.detail.get("origin").filter(|value| !value.is_null()) {
            let origin = serde_json::from_value::<GatingOrigin>(value.clone())
                .map_err(|_| InvalidSnapshot("invalid audit origin"))?;
            if !valid_gating_origin(&origin) {
                return Err(InvalidSnapshot("invalid audit origin"));
            }
        }
    }
    Ok(())
}

impl MobkitRuntimeHandle {
    /// Export canonical gating state after applying owner timeout transitions.
    pub fn gating_state_snapshot(&mut self) -> GatingStateSnapshot {
        self.refresh_gating_timeouts();
        GatingStateSnapshot {
            version: 2,
            owner_epoch: self.gating_epoch.clone(),
            next_sequence: self.gating_sequence,
            pending: self
                .gating_pending_order
                .iter()
                .filter_map(|id| self.gating_pending.get(id).cloned())
                .collect(),
            audit: self.gating_audit.clone(),
        }
    }

    /// Restore trusted host persistence before any gating action is evaluated.
    /// Validation is atomic, and expired entries resolve through the existing
    /// owner timeout path instead of becoming silently approved or renewed.
    /// Restored IDs stay as they are; this owner keeps minting under its own
    /// epoch and never continues the snapshot's sequence.
    pub fn restore_gating_state(
        &mut self,
        snapshot: GatingStateSnapshot,
    ) -> Result<(), GatingStateRestoreError> {
        if self.gating_sequence != 0
            || !self.gating_pending.is_empty()
            || !self.gating_pending_order.is_empty()
            || !self.gating_audit.is_empty()
        {
            return Err(GatingStateRestoreError::RuntimeNotPristine);
        }
        validate_gating_snapshot(&snapshot)?;
        self.gating_pending_order = snapshot
            .pending
            .iter()
            .map(|entry| entry.pending_id.clone())
            .collect();
        self.gating_pending = snapshot
            .pending
            .into_iter()
            .map(|entry| (entry.pending_id.clone(), entry))
            .collect();
        self.gating_audit = snapshot.audit;
        self.refresh_gating_timeouts();
        Ok(())
    }

    /// Whether this owner can mint `count` more gate identities.
    fn gating_ids_available(&self, count: u64) -> Result<(), GatingIdUnavailable> {
        if self.gating_epoch.is_none() {
            return Err(GatingIdUnavailable::EpochUnavailable);
        }
        self.gating_sequence
            .checked_add(count)
            .map(|_| ())
            .ok_or(GatingIdUnavailable::SequenceExhausted)
    }
    /// Mint the next gate identity of `kind` (`action`, `pending` or `audit`)
    /// under this owner's epoch. Exhaustion fails; it never wraps or repeats.
    fn mint_gating_id(&mut self, kind: &str) -> Result<String, GatingIdUnavailable> {
        let epoch = self
            .gating_epoch
            .as_deref()
            .ok_or(GatingIdUnavailable::EpochUnavailable)?;
        let sequence = self.gating_sequence;
        let next = sequence
            .checked_add(1)
            .ok_or(GatingIdUnavailable::SequenceExhausted)?;
        let id = format!("gate-{kind}-{GATING_ID_FORMAT}-{epoch}-{sequence:06}");
        self.gating_sequence = next;
        Ok(id)
    }
    fn append_gating_audit(&mut self, entry: GatingAuditEntry) -> Result<(), GatingIdUnavailable> {
        let audit_id = self.mint_gating_id("audit")?;
        self.record_gating_audit(audit_id, entry);
        Ok(())
    }
    fn record_gating_audit(&mut self, audit_id: String, mut entry: GatingAuditEntry) {
        entry.audit_id = audit_id;
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
            // Without an audit identity the expiry cannot be recorded, so the
            // entry stays pending; deciding it is refused the same way.
            let audit_id = match self.mint_gating_id("audit") {
                Ok(audit_id) => audit_id,
                Err(unavailable) => {
                    tracing::warn!(%pending_id, %unavailable, "gating timeout not recorded");
                    return;
                }
            };
            if let Some(expired_entry) = self.gating_pending.remove(&pending_id) {
                self.gating_pending_order
                    .retain(|candidate| candidate != &pending_id);
                self.record_gating_audit(
                    audit_id,
                    GatingAuditEntry {
                        audit_id: String::new(),
                        timestamp_ms: 0,
                        event_type: "timeout_fallback".to_string(),
                        action_id: expired_entry.action_id.clone(),
                        pending_id: Some(pending_id.clone()),
                        actor_id: expired_entry.actor_id,
                        risk_tier: expired_entry.risk_tier,
                        outcome: GatingOutcome::SafeDraft,
                        detail: serde_json::json!({
                            "fallback": "safe_draft",
                            "reason": "approval_timeout",
                            "origin": expired_entry.origin,
                        }),
                    },
                );
                self.gating_resolution_observers
                    .notify(&GatingResolutionNotice {
                        pending_id,
                        action_id: expired_entry.action_id,
                        approved: false,
                        next_pending_id: None,
                        cause: "timeout_fallback".to_string(),
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
        self.evaluate_gating_action_with_origin(request, None)
    }

    /// Evaluate at a trusted host action boundary with optional conversation
    /// provenance. Callers must derive origin from their authenticated action
    /// context. The ordinary RPC method never accepts an origin claim.
    pub fn evaluate_gating_action_with_origin(
        &mut self,
        request: GatingEvaluateRequest,
        origin: Option<GatingOrigin>,
    ) -> GatingEvaluateResult {
        let action = request.action.trim().to_string();
        let actor_id = request.actor_id.trim().to_string();
        let risk_tier = request.risk_tier.clone();
        self.evaluate_gating_action_minting(request, origin)
            .unwrap_or_else(|unavailable| GatingEvaluateResult {
                action_id: String::new(),
                action,
                actor_id,
                risk_tier,
                outcome: GatingOutcome::SafeDraft,
                pending_id: None,
                fallback_reason: Some(unavailable.fallback_reason().to_string()),
            })
    }

    /// Evaluation body. It checks up front that every identity it can mint is
    /// available, so a refusal never leaves a partial pending entry, audit
    /// record or approval notification behind.
    fn evaluate_gating_action_minting(
        &mut self,
        request: GatingEvaluateRequest,
        origin: Option<GatingOrigin>,
    ) -> Result<GatingEvaluateResult, GatingIdUnavailable> {
        // Optional correlation metadata must not erase an identity access boundary.
        let origin = origin.map(|mut origin| {
            origin.conversation_id = origin
                .conversation_id
                .filter(|value| !value.trim().is_empty());
            origin.interaction_id = origin
                .interaction_id
                .filter(|value| !value.trim().is_empty());
            origin
        });
        self.refresh_gating_timeouts();
        self.gating_ids_available(GATING_IDS_PER_EVALUATION)?;
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
        let action_id = self.mint_gating_id("action")?;
        let risk_tier = request.risk_tier.clone();
        if origin
            .as_ref()
            .is_some_and(|origin| !valid_gating_origin(origin))
        {
            // Refuse malformed supplied identity without creating an unattributed
            // pending or audit row that global gating grants could disclose.
            return Ok(GatingEvaluateResult {
                action_id,
                action,
                actor_id,
                risk_tier,
                outcome: GatingOutcome::SafeDraft,
                pending_id: None,
                fallback_reason: Some("invalid_gating_origin".to_string()),
            });
        }

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
                        "action": action,
                        "reference": {
                            "entity": entity,
                            "topic": topic,
                        },
                        "missing_context": {
                            "entity": entity.is_none(),
                            "topic": topic.is_none(),
                        },
                        "conflict_count": self.memory_conflicts.len(),
                        "origin": origin,
                    }),
                })?;
                return Ok(GatingEvaluateResult {
                    action_id,
                    action,
                    actor_id,
                    risk_tier,
                    outcome: GatingOutcome::SafeDraft,
                    pending_id: None,
                    fallback_reason: Some("memory_conflict_context_missing".to_string()),
                });
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
                            "origin": origin,
                            "reference": {
                                "entity": entity,
                                "topic": topic,
                            },
                        }),
                    })?;
                    return Ok(GatingEvaluateResult {
                        action_id,
                        action,
                        actor_id,
                        risk_tier,
                        outcome: GatingOutcome::SafeDraft,
                        pending_id: None,
                        fallback_reason: Some("memory_conflict_lookup_failed".to_string()),
                    });
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
                        "action": action,
                        "reference": {
                            "entity": entity,
                            "topic": topic,
                        },
                        "conflict": conflict,
                        "origin": origin,
                    }),
                })?;
                return Ok(GatingEvaluateResult {
                    action_id,
                    action,
                    actor_id,
                    risk_tier,
                    outcome: GatingOutcome::SafeDraft,
                    pending_id: None,
                    fallback_reason: Some("memory_conflict".to_string()),
                });
            }
        }

        Ok(match request.risk_tier {
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
                        "origin": origin,
                        "rationale": request.rationale,
                        "action": action,
                    }),
                })?;
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
                        "origin": origin,
                        "rationale": request.rationale,
                        "action": action,
                    }),
                })?;
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
                let pending_id = self.mint_gating_id("pending")?;
                let created_at_ms = current_time_ms();
                // Clamp both ends. The upper bound stops a deadline that
                // saturates past `u64::MAX` from never expiring (the
                // timeout-fallback guarantee must stay reachable). The lower
                // bound stops a tiny/zero `approval_timeout_ms` from minting a
                // pending entry that `refresh_gating_timeouts` treats as already
                // expired on the next RPC, which would make R3 approval
                // impossible to complete. Absent (`None`) still uses the
                // default, which already sits inside the clamp.
                let timeout_ms = request
                    .approval_timeout_ms
                    .unwrap_or(GATING_APPROVAL_TIMEOUT_DEFAULT_MS)
                    .clamp(
                        GATING_APPROVAL_TIMEOUT_MIN_MS,
                        GATING_APPROVAL_TIMEOUT_MAX_MS,
                    );
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
                                        "pending_id": pending_id,
                                        "action_id": action_id,
                                        "action": action,
                                        "actor_id": actor_id,
                                        "risk_tier": risk_tier,
                                        "requested_approver": requested_approver,
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
                    rationale: request.rationale,
                    origin,
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
                        "requested_approver": pending_entry.requested_approver,
                        "approval_recipient": pending_entry.approval_recipient,
                        "approval_channel": pending_entry.approval_channel,
                        "approval_route_id": pending_entry.approval_route_id,
                        "approval_delivery_id": pending_entry.approval_delivery_id,
                        "approval_notification_error": approval_notification_error,
                        "deadline_at_ms": pending_entry.deadline_at_ms,
                        "action": action,
                        "origin": pending_entry.origin,
                    }),
                })?;
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
        })
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
        // Checked before the entry is taken, so a refusal leaves it pending.
        self.gating_ids_available(GATING_IDS_PER_DECISION)
            .map_err(GatingDecideError::IdsUnavailable)?;
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
            self.upsert_gating_pending_entry(pending_entry);
            return Err(GatingDecideError::SelfApprovalForbidden);
        }
        if let Some(expected_approver) = pending_entry.requested_approver.as_deref()
            && expected_approver != approver_id
        {
            let expected = expected_approver.to_string();
            self.upsert_gating_pending_entry(pending_entry);
            return Err(GatingDecideError::ApproverMismatch {
                expected,
                provided: approver_id,
            });
        }

        let mut next_pending_id = None;
        let (outcome, event_type) = match decision {
            GatingDecision::Approve => (GatingOutcome::Allowed, "approval_decided"),
            GatingDecision::Reject => (GatingOutcome::SafeDraft, "rejection_decided"),
            GatingDecision::Escalate => {
                let successor_pending_id = self
                    .mint_gating_id("pending")
                    .map_err(GatingDecideError::IdsUnavailable)?;
                let successor_entry = GatingPendingEntry {
                    pending_id: successor_pending_id.clone(),
                    action_id: pending_entry.action_id.clone(),
                    action: pending_entry.action.clone(),
                    actor_id: pending_entry.actor_id.clone(),
                    risk_tier: pending_entry.risk_tier.clone(),
                    requested_approver: None,
                    approval_recipient: pending_entry.approval_recipient.clone(),
                    approval_channel: pending_entry.approval_channel.clone(),
                    approval_route_id: None,
                    approval_delivery_id: None,
                    created_at_ms: current_time_ms(),
                    deadline_at_ms: pending_entry.deadline_at_ms,
                    rationale: pending_entry.rationale.clone(),
                    origin: pending_entry.origin.clone(),
                };
                self.upsert_gating_pending_entry(successor_entry.clone());
                next_pending_id = Some(successor_pending_id.clone());
                self.append_gating_audit(GatingAuditEntry {
                    audit_id: String::new(),
                    timestamp_ms: 0,
                    event_type: "pending_created".to_string(),
                    action_id: successor_entry.action_id.clone(),
                    pending_id: Some(successor_pending_id),
                    actor_id: successor_entry.actor_id.clone(),
                    risk_tier: successor_entry.risk_tier.clone(),
                    outcome: GatingOutcome::PendingApproval,
                    detail: serde_json::json!({
                        "escalated_from_pending_id": pending_id.clone(),
                        "requested_approver": successor_entry.requested_approver,
                        "approval_recipient": successor_entry.approval_recipient,
                        "approval_channel": successor_entry.approval_channel,
                        "approval_route_id": successor_entry.approval_route_id,
                        "approval_delivery_id": successor_entry.approval_delivery_id,
                        "deadline_at_ms": successor_entry.deadline_at_ms,
                        "action": successor_entry.action,
                        "origin": successor_entry.origin,
                    }),
                })
                .map_err(GatingDecideError::IdsUnavailable)?;
                (GatingOutcome::PendingApproval, "escalation_decided")
            }
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
                "decision": decision,
                "reason": reason,
                "approval_route_id": pending_entry.approval_route_id,
                "approval_delivery_id": pending_entry.approval_delivery_id,
                "next_pending_id": next_pending_id,
                "origin": pending_entry.origin,
            }),
        })
        .map_err(GatingDecideError::IdsUnavailable)?;
        self.gating_resolution_observers
            .notify(&GatingResolutionNotice {
                pending_id: pending_id.clone(),
                action_id: pending_entry.action_id.clone(),
                approved: matches!(decision, GatingDecision::Approve),
                next_pending_id: next_pending_id.clone(),
                cause: event_type.to_string(),
            });
        Ok(GatingDecisionResult {
            pending_id,
            action_id: pending_entry.action_id,
            approver_id,
            decision,
            outcome,
            decided_at_ms,
            reason,
            next_pending_id,
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

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod gating_identity_tests {
    use super::*;

    fn owner() -> MobkitRuntimeHandle {
        crate::start_mobkit_runtime(
            crate::MobKitConfig {
                modules: vec![],
                discovery: crate::DiscoverySpec {
                    namespace: "gating-identity".to_string(),
                    modules: vec![],
                },
                pre_spawn: vec![],
            },
            vec![],
            Duration::from_secs(1),
        )
        .expect("runtime starts")
    }

    fn r3() -> GatingEvaluateRequest {
        GatingEvaluateRequest {
            action: "Deploy".to_string(),
            actor_id: "worker".to_string(),
            risk_tier: GatingRiskTier::R3,
            rationale: None,
            requested_approver: None,
            approval_recipient: None,
            approval_channel: None,
            approval_timeout_ms: None,
            entity: None,
            topic: None,
        }
    }

    #[test]
    fn minted_ids_carry_the_owner_epoch() {
        let mut first = owner();
        let epoch = first.gating_epoch.clone().expect("epoch");
        assert!(valid_gating_epoch(&epoch));
        let result = first.evaluate_gating_action(r3());
        assert_eq!(result.action_id, format!("gate-action-v2-{epoch}-000000"));
        assert_eq!(
            result.pending_id,
            Some(format!("gate-pending-v2-{epoch}-000001"))
        );
        assert_eq!(
            first.gating_audit_entries(1)[0].audit_id,
            format!("gate-audit-v2-{epoch}-000002")
        );
        assert_ne!(owner().gating_epoch, first.gating_epoch);
    }

    #[test]
    fn exhausted_sequence_refuses_typed_and_never_reuses() {
        let mut owner = owner();
        let pending_id = owner
            .evaluate_gating_action(r3())
            .pending_id
            .expect("pends");
        let before = owner.gating_state_snapshot();

        owner.gating_sequence = u64::MAX - 1;
        let refused = owner.evaluate_gating_action(r3());
        assert_eq!(refused.outcome, GatingOutcome::SafeDraft);
        assert_eq!(
            refused.fallback_reason.as_deref(),
            Some("gating_sequence_exhausted")
        );
        assert!(refused.pending_id.is_none());
        assert!(refused.action_id.is_empty());
        assert_eq!(owner.gating_sequence, u64::MAX - 1);
        assert_eq!(owner.list_gating_pending(), before.pending);
        assert_eq!(owner.gating_audit_entries(512), before.audit);
        assert!(matches!(
            owner.decide_gating_action(GatingDecideRequest {
                pending_id,
                approver_id: "operator".to_string(),
                decision: GatingDecision::Approve,
                reason: None,
            }),
            Err(GatingDecideError::IdsUnavailable(
                GatingIdUnavailable::SequenceExhausted
            ))
        ));
        assert_eq!(owner.list_gating_pending(), before.pending);

        // The last identities are minted once, then minting fails.
        owner.gating_sequence = u64::MAX - GATING_IDS_PER_EVALUATION;
        assert!(owner.evaluate_gating_action(r3()).pending_id.is_some());
        assert_eq!(owner.gating_sequence, u64::MAX);
        assert_eq!(
            owner.mint_gating_id("audit"),
            Err(GatingIdUnavailable::SequenceExhausted)
        );
        assert_eq!(owner.gating_sequence, u64::MAX);
    }

    #[test]
    fn rpc_decide_reports_unavailable_ids_as_a_typed_internal_error() {
        let mut owner = owner();
        let pending_id = owner
            .evaluate_gating_action(r3())
            .pending_id
            .expect("pends");
        let decide = |pending_id: &str| {
            serde_json::json!({
                "jsonrpc": "2.0", "id": 1, "method": "mobkit/gating/decide",
                "params": {
                    "pending_id": pending_id, "approver_id": "operator", "decision": "approve",
                },
            })
            .to_string()
        };
        let unknown: Value = serde_json::from_str(&crate::handle_mobkit_rpc_json(
            &mut owner,
            &decide("gate-unknown"),
            Duration::from_secs(1),
        ))
        .expect("response");
        assert_eq!(unknown["error"]["code"], serde_json::json!(-32602));

        owner.gating_sequence = u64::MAX - 1;
        let before = owner.list_gating_pending();
        let refused: Value = serde_json::from_str(&crate::handle_mobkit_rpc_json(
            &mut owner,
            &decide(&pending_id),
            Duration::from_secs(1),
        ))
        .expect("response");
        assert_eq!(refused["error"]["code"], serde_json::json!(-32603));
        assert_eq!(
            refused["error"]["data"],
            serde_json::json!({
                "error": "gating_ids_unavailable",
                "reason": "gating_sequence_exhausted",
            })
        );
        assert_eq!(owner.list_gating_pending(), before);
    }

    #[test]
    fn owner_without_an_epoch_cannot_mint() {
        let mut owner = owner();
        owner.gating_epoch = None;
        let refused = owner.evaluate_gating_action(r3());
        assert_eq!(refused.outcome, GatingOutcome::SafeDraft);
        assert_eq!(
            refused.fallback_reason.as_deref(),
            Some("gating_identity_unavailable")
        );
        assert!(owner.list_gating_pending().is_empty());
        assert!(owner.gating_audit_entries(512).is_empty());
        assert_eq!(owner.gating_sequence, 0);
    }
}
