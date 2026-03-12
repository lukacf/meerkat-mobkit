//! Parameter parsing for gating RPC methods.

use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum GatingParamsError {
    ParamsMustBeObject,
    ActionRequired,
    ActorRequired,
    RiskTierRequired,
    UnknownRiskTier(String),
    RationaleMustBeString,
    RequestedApproverMustBeString,
    ApprovalRecipientMustBeString,
    ApprovalChannelMustBeString,
    ApprovalTimeoutMustBeInteger,
    EntityMustBeString,
    TopicMustBeString,
    PendingIdRequired,
    ApproverIdRequired,
    DecisionRequired,
    UnknownDecision(String),
    ReasonMustBeString,
    LimitOutOfRange,
    Decision(GatingDecideError),
}

impl GatingParamsError {
    pub(super) fn message(&self) -> String {
        match self {
            GatingParamsError::ParamsMustBeObject => "params must be a JSON object".to_string(),
            GatingParamsError::ActionRequired => "action must be a non-empty string".to_string(),
            GatingParamsError::ActorRequired => "actor_id must be a non-empty string".to_string(),
            GatingParamsError::RiskTierRequired => {
                "risk_tier must be one of: r0, r1, r2, r3".to_string()
            }
            GatingParamsError::UnknownRiskTier(tier) => {
                format!("risk_tier '{tier}' is unsupported (allowed: r0, r1, r2, r3)")
            }
            GatingParamsError::RationaleMustBeString => "rationale must be a string".to_string(),
            GatingParamsError::RequestedApproverMustBeString => {
                "requested_approver must be a string".to_string()
            }
            GatingParamsError::ApprovalRecipientMustBeString => {
                "approval_recipient must be a string".to_string()
            }
            GatingParamsError::ApprovalChannelMustBeString => {
                "approval_channel must be a string".to_string()
            }
            GatingParamsError::ApprovalTimeoutMustBeInteger => {
                "approval_timeout_ms must be a non-negative integer".to_string()
            }
            GatingParamsError::EntityMustBeString => {
                "entity must be a string when provided".to_string()
            }
            GatingParamsError::TopicMustBeString => {
                "topic must be a string when provided".to_string()
            }
            GatingParamsError::PendingIdRequired => {
                "pending_id must be a non-empty string".to_string()
            }
            GatingParamsError::ApproverIdRequired => {
                "approver_id must be a non-empty string".to_string()
            }
            GatingParamsError::DecisionRequired => {
                "decision must be either 'approve' or 'reject'".to_string()
            }
            GatingParamsError::UnknownDecision(decision) => {
                format!("decision '{decision}' is unsupported (allowed: approve, reject)")
            }
            GatingParamsError::ReasonMustBeString => "reason must be a string".to_string(),
            GatingParamsError::LimitOutOfRange => {
                "limit must be an integer between 1 and 500".to_string()
            }
            GatingParamsError::Decision(GatingDecideError::UnknownPendingId(pending_id)) => {
                format!("pending_id '{pending_id}' was not found")
            }
            GatingParamsError::Decision(GatingDecideError::SelfApprovalForbidden) => {
                "approver_id cannot self-approve the action actor".to_string()
            }
            GatingParamsError::Decision(GatingDecideError::ApproverMismatch {
                expected,
                provided,
            }) => {
                format!("approver_id '{provided}' does not match requested_approver '{expected}'")
            }
        }
    }
}

pub(super) fn parse_gating_evaluate_params(
    params: &Value,
) -> Result<GatingEvaluateRequest, GatingParamsError> {
    let object = params
        .as_object()
        .ok_or(GatingParamsError::ParamsMustBeObject)?;
    let action = object
        .get("action")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(GatingParamsError::ActionRequired)?;
    let actor_id = object
        .get("actor_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(GatingParamsError::ActorRequired)?;
    let risk_tier = object
        .get("risk_tier")
        .and_then(Value::as_str)
        .ok_or(GatingParamsError::RiskTierRequired)?;
    let risk_tier = parse_gating_risk_tier(risk_tier)?;
    let rationale = match object.get("rationale") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::RationaleMustBeString)?
                .to_string(),
        ),
    };
    let requested_approver = match object.get("requested_approver") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::RequestedApproverMustBeString)?
                .to_string(),
        ),
    };
    let approval_recipient = match object.get("approval_recipient") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::ApprovalRecipientMustBeString)?
                .to_string(),
        ),
    };
    let approval_channel = match object.get("approval_channel") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::ApprovalChannelMustBeString)?
                .to_string(),
        ),
    };
    let approval_timeout_ms = match object.get("approval_timeout_ms") {
        None => None,
        Some(value) => Some(
            value
                .as_u64()
                .ok_or(GatingParamsError::ApprovalTimeoutMustBeInteger)?,
        ),
    };
    let entity = match object.get("entity") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::EntityMustBeString)?
                .to_string(),
        ),
    };
    let topic = match object.get("topic") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::TopicMustBeString)?
                .to_string(),
        ),
    };

    Ok(GatingEvaluateRequest {
        action: action.to_string(),
        actor_id: actor_id.to_string(),
        risk_tier,
        rationale,
        requested_approver,
        approval_recipient,
        approval_channel,
        approval_timeout_ms,
        entity,
        topic,
    })
}

pub(super) fn parse_gating_pending_params(params: &Value) -> Result<(), GatingParamsError> {
    if params.is_null() || params.is_object() {
        return Ok(());
    }
    Err(GatingParamsError::ParamsMustBeObject)
}

pub(super) fn parse_gating_decide_params(
    params: &Value,
) -> Result<GatingDecideRequest, GatingParamsError> {
    let object = params
        .as_object()
        .ok_or(GatingParamsError::ParamsMustBeObject)?;
    let pending_id = object
        .get("pending_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(GatingParamsError::PendingIdRequired)?;
    let approver_id = object
        .get("approver_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or(GatingParamsError::ApproverIdRequired)?;
    let decision = object
        .get("decision")
        .and_then(Value::as_str)
        .ok_or(GatingParamsError::DecisionRequired)?;
    let decision = parse_gating_decision(decision)?;
    let reason = match object.get("reason") {
        None => None,
        Some(value) => Some(
            value
                .as_str()
                .ok_or(GatingParamsError::ReasonMustBeString)?
                .to_string(),
        ),
    };

    Ok(GatingDecideRequest {
        pending_id: pending_id.to_string(),
        approver_id: approver_id.to_string(),
        decision,
        reason,
    })
}

pub(super) fn parse_gating_audit_params(params: &Value) -> Result<usize, GatingParamsError> {
    if params.is_null() {
        return Ok(50);
    }
    let object = params
        .as_object()
        .ok_or(GatingParamsError::ParamsMustBeObject)?;
    let limit = match object.get("limit") {
        None => 50,
        Some(value) => {
            let Some(raw_limit) = value.as_u64() else {
                return Err(GatingParamsError::LimitOutOfRange);
            };
            if !(1..=500).contains(&raw_limit) {
                return Err(GatingParamsError::LimitOutOfRange);
            }
            raw_limit as usize
        }
    };
    Ok(limit)
}

fn parse_gating_risk_tier(raw: &str) -> Result<GatingRiskTier, GatingParamsError> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "r0" => Ok(GatingRiskTier::R0),
        "r1" => Ok(GatingRiskTier::R1),
        "r2" => Ok(GatingRiskTier::R2),
        "r3" => Ok(GatingRiskTier::R3),
        "" => Err(GatingParamsError::RiskTierRequired),
        _ => Err(GatingParamsError::UnknownRiskTier(raw.to_string())),
    }
}

fn parse_gating_decision(raw: &str) -> Result<GatingDecision, GatingParamsError> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "approve" => Ok(GatingDecision::Approve),
        "reject" => Ok(GatingDecision::Reject),
        "" => Err(GatingParamsError::DecisionRequired),
        _ => Err(GatingParamsError::UnknownDecision(raw.to_string())),
    }
}
