"""Decision service SDK surface: request builders, RPC method name, typed result."""

import pytest
from unittest.mock import MagicMock

from meerkat_mobkit import (
    DecisionJudgment,
    DecisionResult,
    binary_question,
    choose_one_question,
    grade_question,
)


def make_mock_mob_handle(rpc_responses=None):
    from meerkat_mobkit.runtime import MobHandle

    runtime = MagicMock()
    calls = []

    async def mock_rpc(method, params=None):
        calls.append((method, params))
        if rpc_responses and method in rpc_responses:
            return rpc_responses[method]
        return {}

    runtime._rpc = mock_rpc
    handle = MobHandle.__new__(MobHandle)
    handle._runtime = runtime
    return handle, calls


WIRE_RESULT = {
    "contract": "v1",
    "route": {"backend": "jev", "endpoint": "https://api.typesafe.ai/v1/systemone",
              "requested_model": "jev-latest", "served_model": "jev-1.13.0"},
    "judgments": {
        "is_urgent": {"judgment": {"kind": "binary", "form": "native_probability", "yes": 0.95}},
        "department": {
            "judgment": {"kind": "choice", "form": "selected", "option": "billing"},
            "native_signals": [{"signal": "choice_distribution", "backend": "jev",
                                "probabilities": {"billing": 0.88, "technical": 0.12},
                                "confidence": 0.81}],
        },
        "frustration": {"judgment": {"kind": "grade", "form": "native_weighted", "position": 1.05}},
        "any_fit": {"judgment": {"kind": "choice", "form": "abstain"}},
    },
    "accounting": {"kind": "measured", "input_tokens": 296, "output_tokens": 20},
    "budget": {"kind": "not_issued"},
    "attempts": 1,
}


def test_question_builders_emit_the_typed_wire_shape():
    assert binary_question("is_urgent", "Does this convey urgency?") == {
        "kind": "binary",
        "id": "is_urgent",
        "instructions": "Does this convey urgency?",
    }
    assert binary_question("q", "x", criteria={"yes": "a", "no": "b"})["criteria"] == {
        "yes": "a",
        "no": "b",
    }
    choice = choose_one_question("department", "Which team?", {"billing": "Payments", "technical": "Bugs"})
    assert choice["kind"] == "choose_one"
    assert choice["options"] == [
        {"id": "billing", "description": "Payments"},
        {"id": "technical", "description": "Bugs"},
    ]
    grade = grade_question("frustration", "How frustrated?", ["Calm", "Frustrated", "Very angry"])
    assert grade["kind"] == "grade"
    assert grade["levels"][2] == {"description": "Very angry"}


@pytest.mark.asyncio
async def test_decide_calls_the_rpc_and_types_the_result():
    handle, calls = make_mock_mob_handle({"mobkit/decision/evaluate": WIRE_RESULT})

    result = await handle.decide(
        "Help! My payouts have been failing for 3 days.",
        [binary_question("is_urgent", "Does this convey urgency?")],
        task="Triage",
    )

    assert calls[0][0] == "mobkit/decision/evaluate"
    assert calls[0][1] == {
        "state": "Help! My payouts have been failing for 3 days.",
        "questions": [{"kind": "binary", "id": "is_urgent", "instructions": "Does this convey urgency?"}],
        "task": "Triage",
    }
    assert isinstance(result, DecisionResult)
    assert result.contract == "v1"
    assert result.route["backend"] == "jev"
    assert result.attempts == 1
    assert result.accounting["kind"] == "measured"
    assert result.budget == {"kind": "not_issued"}

    urgent = result.judgments["is_urgent"]
    assert isinstance(urgent, DecisionJudgment)
    assert urgent.form == "native_probability"
    assert urgent.probability_yes == 0.95
    assert urgent.answer is None, "no threshold is applied by the SDK"

    department = result.judgments["department"]
    assert department.option == "billing"
    assert department.native_signals[0]["probabilities"]["billing"] == 0.88

    assert result.judgments["frustration"].weighted_position == 1.05
    assert result.judgments["any_fit"].is_abstain


@pytest.mark.asyncio
async def test_decide_omits_task_when_not_given():
    handle, calls = make_mock_mob_handle({"mobkit/decision/evaluate": WIRE_RESULT})
    await handle.decide({"records": []}, [binary_question("q", "x")])
    assert "task" not in calls[0][1]
    assert calls[0][1]["state"] == {"records": []}
