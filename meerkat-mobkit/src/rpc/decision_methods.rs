//! `mobkit/decision/evaluate` dispatch for the unified stdin RPC (the SDK
//! transport).
//!
//! A thin skin over the host-composed [`meerkat_decision::DecisionService`]:
//! the params are the typed [`DecisionRequest`] wire shape, the result is the
//! typed [`meerkat_decision::DecisionResult`], and every failure keeps its
//! typed class. The surface adds no thresholds, no defaults, and no route
//! election; a runtime without a composed service answers the SDKs' reserved
//! capability-unavailable code rather than pretending.

use meerkat_decision::{DecisionAdmission, DecisionError, DecisionRequest, DecisionService};
use serde_json::Value;

use super::{CAPABILITY_UNAVAILABLE_CODE, JsonRpcError};

/// The evaluate RPC method name.
pub const DECISION_EVALUATE_METHOD: &str = "mobkit/decision/evaluate";

/// Parse the typed request out of the JSON-RPC params.
pub(crate) fn parse_decision_evaluate_params(params: &Value) -> Result<DecisionRequest, String> {
    if !params.is_object() {
        return Err("params must be an object with `state` and `questions`".to_string());
    }
    serde_json::from_value(params.clone()).map_err(|error| error.to_string())
}

/// The typed refusal when no decision service is composed into this runtime.
pub(crate) fn decision_service_unavailable_error() -> JsonRpcError {
    JsonRpcError {
        code: CAPABILITY_UNAVAILABLE_CODE,
        message: "decision service is not composed into this runtime: install one with \
                  UnifiedRuntimeBuilder::decision_service (see meerkat::build_decision_service)"
            .to_string(),
        data: Some(serde_json::json!({ "kind": "decision_service_unavailable" })),
    }
}

/// Map a typed decision failure onto the JSON-RPC error vocabulary without
/// collapsing its class: invalid requests are param errors, unavailability
/// is the reserved capability code, everything else is a server error whose
/// `data` carries the full typed [`DecisionError`].
pub(crate) fn decision_error_to_rpc(error: &DecisionError) -> JsonRpcError {
    let data = serde_json::to_value(error).unwrap_or(Value::Null);
    let code = match error {
        DecisionError::InvalidRequest(_) => -32602,
        DecisionError::Unavailable(_) => CAPABILITY_UNAVAILABLE_CODE,
        DecisionError::BackendFailure(_)
        | DecisionError::InvalidAnswer(_)
        | DecisionError::DeadlineExceeded { .. }
        | DecisionError::BudgetRefused { .. } => -32000,
    };
    JsonRpcError {
        code,
        message: format!("{}: {error}", error.code().as_str()),
        data: Some(data),
    }
}

/// Evaluate one request under host admission and project the typed result.
pub(crate) async fn evaluate_decision(
    service: &DecisionService,
    request: DecisionRequest,
) -> Result<Value, JsonRpcError> {
    let result = service
        .evaluate(&DecisionAdmission::host_unbudgeted(), request)
        .await
        .map_err(|error| decision_error_to_rpc(&error))?;
    serde_json::to_value(result).map_err(|error| JsonRpcError {
        code: -32603,
        message: format!("decision result failed to serialize: {error}"),
        data: None,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use meerkat_decision::{
        AnswerValidationError, BackendFailure, DecisionUnavailableReason, QuestionId,
        RequestValidationError,
    };

    use super::*;

    #[test]
    fn params_must_be_a_typed_request_object() {
        assert!(parse_decision_evaluate_params(&Value::Null).is_err());
        assert!(parse_decision_evaluate_params(&serde_json::json!("state")).is_err());
        let parsed = parse_decision_evaluate_params(&serde_json::json!({
            "state": "Help! My payouts have been failing for 3 days.",
            "questions": [
                {"kind": "binary", "id": "is_urgent", "instructions": "Does this convey urgency?"}
            ]
        }))
        .unwrap();
        assert_eq!(parsed.questions.len(), 1);
        assert!(
            parse_decision_evaluate_params(&serde_json::json!({
                "state": 5,
                "questions": []
            }))
            .is_err(),
            "scalar non-string state is rejected at the boundary"
        );
    }

    #[test]
    fn error_classes_keep_distinct_codes_and_typed_data() {
        let invalid = decision_error_to_rpc(&DecisionError::InvalidRequest(
            RequestValidationError::NoQuestions,
        ));
        assert_eq!(invalid.code, -32602);
        assert_eq!(invalid.data.as_ref().unwrap()["code"], "invalid_request");

        let unavailable = decision_error_to_rpc(&DecisionError::Unavailable(
            DecisionUnavailableReason::Disabled,
        ));
        assert_eq!(unavailable.code, CAPABILITY_UNAVAILABLE_CODE);

        let backend =
            decision_error_to_rpc(&DecisionError::BackendFailure(BackendFailure::RateLimited));
        assert_eq!(backend.code, -32000);
        assert_eq!(backend.data.as_ref().unwrap()["reason"], "rate_limited");
        assert!(backend.message.starts_with("backend_failure:"));

        let answer = decision_error_to_rpc(&DecisionError::InvalidAnswer(
            AnswerValidationError::MissingAnswer {
                question: QuestionId::new("q").unwrap(),
            },
        ));
        assert_eq!(answer.code, -32000);
        assert_eq!(answer.data.as_ref().unwrap()["code"], "invalid_answer");

        assert_eq!(
            decision_service_unavailable_error().code,
            CAPABILITY_UNAVAILABLE_CODE
        );
    }
}
