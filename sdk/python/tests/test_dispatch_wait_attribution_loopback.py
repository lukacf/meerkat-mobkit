"""``*_and_wait`` never attributes another turn and never loses its admission.

The incident (MobKit 0.8.42): ``dispatch_text_and_wait`` asked for
``track_turn``. With no ticket it silently fell back to the identity-wide
cursor wait, which any other peer or scheduled turn can satisfy, and an
observation error after a successful admission lost the ``DispatchResult``
(only a local variable), inviting callers to redispatch business work.

These tests drive the real SDK runtime and its persistent stdio JSON-RPC
transport against a scripted stand-in gateway process over loopback pipes.
The stand-in serves the actual wire methods (``mobkit/dispatch``,
``mobkit/wait_for_turn``, ...) and logs every request it receives, so each
test can count dispatches and prove which observation methods ran. The
gateway's own ticket semantics are covered by the Rust
``identity_first_turn_tickets`` tests; the shipped gateway cannot run a turn
here without a live model provider.
"""
from __future__ import annotations

import asyncio
import json
import sys
from pathlib import Path

import pytest

from meerkat_mobkit.builder import MobKit
from meerkat_mobkit.errors import (
    PostAdmissionObservationError,
    RpcError,
    TurnTrackingUnavailableError,
    TurnTrackingUnavailableWarning,
    TurnUnknownError,
    TurnWaitTimeoutError,
)
from meerkat_mobkit.identity_first_models import AwaitedTurn, DispatchResult
from meerkat_mobkit.runtime import MobKitRuntime

_IDENTITY = "keeper"
_TICKET = "6f1c2a8e-0d4b-4b7e-9a51-3c2f8d7e1a90"
_SATURATED = {
    "code": -32000,
    "message": "observation_lane_saturated: member_status_observation",
}
# Identity-wide observation methods. A per-admission wait must never call them.
_IDENTITY_WIDE = (
    "mobkit/wait_for_completion",
    "mobkit/completion_cursor",
    "mobkit/inspect_identity",
)

# The stand-in gateway. Each request is handled on its own thread so a held
# server-side wait does not block other requests; EOF on stdin exits at once.
# ``SCENARIO`` maps a method to the responses it walks through (holding the
# last); a step may ``sleep`` before answering or ``crash`` the process. A
# replacement process (the transport restarts a dead gateway) uses the
# ``after_restart`` scenario: it remembers no ticket.
_STAND_IN = r'''
import json, os, sys, threading, time

SCENARIO = json.loads(__SCENARIO__)
LOG = __LOG__
GENERATION_FILE = LOG + ".generation"
try:
    generation = int(open(GENERATION_FILE).read()) + 1
except FileNotFoundError:
    generation = 1
open(GENERATION_FILE, "w").write(str(generation))
script = SCENARIO["first"] if generation == 1 else SCENARIO["after_restart"]
positions = {}
lock = threading.Lock()

def respond(request):
    method = request.get("method")
    params = request.get("params") or {}
    with lock:
        with open(LOG, "a") as log:
            log.write(json.dumps({"generation": generation, "method": method,
                                  "params": params}) + "\n")
        steps = script.get(method)
        if steps is None:
            step = {"result": {"http_base_url": "http://127.0.0.1:1"}
                    if method == "mobkit/init" else {}}
        else:
            index = min(positions.get(method, 0), len(steps) - 1)
            positions[method] = index + 1
            step = steps[index]
    if step.get("crash"):
        os._exit(1)
    if step.get("sleep"):
        time.sleep(step["sleep"])
    reply = {"jsonrpc": "2.0", "id": request["id"]}
    if "error" in step:
        reply["error"] = step["error"]
    else:
        result = dict(step["result"])
        if method in ("mobkit/wait_for_turn", "mobkit/turn_result"):
            result = {"identity": params.get("identity"),
                      "ticket": params.get("ticket"), **result}
        reply["result"] = result
    with lock:
        sys.stdout.write(json.dumps(reply) + "\n")
        sys.stdout.flush()

for raw in sys.stdin:
    if raw.strip():
        threading.Thread(target=respond, args=(json.loads(raw),), daemon=True).start()
os._exit(0)
'''


def _admitted(ticket: str | None, *, unavailable: str | None = None) -> dict:
    result = {"fencing_token": 3, "completion_baseline": {"epoch": 3, "turns": 4}}
    if ticket is not None:
        result["turn"] = {"ticket": ticket}
    else:
        result["turn"] = None
        if unavailable is not None:
            result["turn_unavailable"] = {
                "code": unavailable, "reason": f"because {unavailable}",
            }
    return result


def _own(output: str) -> dict:
    return {"result": {
        "state": "completed", "wait": "settled", "output_status": "text",
        "output": output, "output_truncated": False,
    }}


_PENDING_AT_DEADLINE = {"result": {"state": "pending", "wait": "timed_out"}}
# What the identity-wide observations would report: a peer's turn completed
# past the dispatch's baseline. Serving it makes any fallback visible.
_PEER_COMPLETED = {
    "mobkit/wait_for_completion": [{"result": {
        "outcome": "completed", "completion_cursor": {"epoch": 3, "turns": 5},
    }}],
    "mobkit/completion_cursor": [{"result": {
        "identity": _IDENTITY, "state": "active",
        "completion_cursor": {"epoch": 3, "turns": 5},
    }}],
    "mobkit/inspect_identity": [{"result": {
        "identity": _IDENTITY, "output_preview": "peer reply", "is_final": False,
        "peer_reachable_count": 0, "completion_cursor": {"epoch": 3, "turns": 5},
    }}],
}


class _StandIn:
    def __init__(self, tmp_path: Path, first: dict, after_restart: dict | None = None):
        self.log = tmp_path / "requests.jsonl"
        scenario = {"first": first, "after_restart": after_restart or {}}
        self.path = tmp_path / "stand_in_gateway.py"
        self.path.write_text(
            f"#!{sys.executable}\n"
            + _STAND_IN.replace("__SCENARIO__", repr(json.dumps(scenario)))
            .replace("__LOG__", repr(str(self.log)))
        )
        self.path.chmod(0o755)

    def requests(self) -> list[dict]:
        if not self.log.exists():
            return []
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def calls(self, method: str) -> list[dict]:
        return [r for r in self.requests() if r["method"] == method]


async def _runtime(stand_in: _StandIn) -> MobKitRuntime:
    runtime = MobKitRuntime(MobKit.builder().gateway(str(stand_in.path))._config)
    await runtime.connect()
    return runtime


@pytest.mark.asyncio
@pytest.mark.timeout(60)
async def test_admission_then_read_failure_retains_the_receipt_and_never_redispatches(tmp_path):
    """The incident: the dispatch is admitted, then the turn observation
    fails. The exact-ticket wait is retried and still answers with this
    turn's own output; the work was dispatched exactly once."""
    stand_in = _StandIn(tmp_path, {
        "mobkit/dispatch": [{"result": _admitted(_TICKET)}],
        "mobkit/wait_for_turn": [
            {"error": _SATURATED}, {"error": _SATURATED}, _own("own reply"),
        ],
        **_PEER_COMPLETED,
    })
    runtime = await _runtime(stand_in)
    try:
        output = await runtime.agent(_IDENTITY).dispatch_text_and_wait(
            "process the incident", idempotency_key="incident-7", timeout=20,
        )
    finally:
        await runtime.shutdown()

    assert output == "own reply"
    assert len(stand_in.calls("mobkit/dispatch")) == 1
    waits = stand_in.calls("mobkit/wait_for_turn")
    assert len(waits) == 3
    assert {w["params"]["ticket"] for w in waits} == {_TICKET}
    assert not [r for r in stand_in.requests() if r["method"] in _IDENTITY_WIDE]


@pytest.mark.asyncio
@pytest.mark.timeout(60)
async def test_persistent_read_failure_raises_typed_with_admission_and_ticket(tmp_path):
    stand_in = _StandIn(tmp_path, {
        "mobkit/dispatch": [{"result": _admitted(_TICKET)}],
        "mobkit/wait_for_turn": [{"error": _SATURATED}],
        **_PEER_COMPLETED,
    })
    runtime = await _runtime(stand_in)
    try:
        with pytest.raises(PostAdmissionObservationError) as raised:
            await runtime.agent(_IDENTITY).dispatch_text_and_wait(
                "process the incident", idempotency_key="incident-7", timeout=1.5,
            )
    finally:
        await runtime.shutdown()

    error = raised.value
    assert isinstance(error.admission, DispatchResult)
    assert error.admission.turn_ticket == _TICKET
    assert error.ticket == _TICKET
    assert error.attempts >= 2
    assert isinstance(error.__cause__, RpcError)
    assert "observation_lane_saturated" in str(error.__cause__)
    assert len(stand_in.calls("mobkit/dispatch")) == 1
    assert not [r for r in stand_in.requests() if r["method"] in _IDENTITY_WIDE]


@pytest.mark.asyncio
@pytest.mark.timeout(60)
async def test_an_unrelated_peer_completion_does_not_satisfy_the_wait(tmp_path):
    """A peer's turn completes on the identity while this dispatch's turn is
    still pending. Only this turn's own settlement answers the wait."""
    stand_in = _StandIn(tmp_path, {
        "mobkit/dispatch": [{"result": _admitted(_TICKET)}],
        "mobkit/wait_for_turn": [
            {"sleep": 0.2, **_PENDING_AT_DEADLINE},
            {"sleep": 0.2, **_PENDING_AT_DEADLINE},
            _own("own reply"),
        ],
        **_PEER_COMPLETED,
    })
    runtime = await _runtime(stand_in)
    try:
        outcome = await runtime.agent(_IDENTITY).dispatch_text_and_wait_outcome(
            "process the incident", timeout=20,
        )
    finally:
        await runtime.shutdown()

    assert (outcome.text, outcome.attributed, outcome.ticket) == ("own reply", True, _TICKET)
    assert len(stand_in.calls("mobkit/wait_for_turn")) == 3
    assert not [r for r in stand_in.requests() if r["method"] in _IDENTITY_WIDE]


@pytest.mark.asyncio
@pytest.mark.timeout(60)
async def test_the_default_mode_waits_identity_wide_typed_as_not_attributed(tmp_path):
    """``autonomous_host`` (the default member mode) returns no ticket by
    construction. The call still waits, so upgrading breaks nothing, but the
    typed outcome says the text is not attributed (here it is the peer's) and
    the warning says so too."""
    stand_in = _StandIn(tmp_path, {
        "mobkit/dispatch": [{"result": _admitted(None, unavailable="autonomous_host")}],
        **_PEER_COMPLETED,
    })
    runtime = await _runtime(stand_in)
    try:
        with pytest.warns(TurnTrackingUnavailableWarning, match="not attributed"):
            outcome = await runtime.agent(_IDENTITY).dispatch_text_and_wait_outcome(
                "process the incident", timeout=5,
            )
    finally:
        await runtime.shutdown()

    assert isinstance(outcome, AwaitedTurn)
    assert (outcome.text, outcome.attributed, outcome.ticket) == ("peer reply", False, None)
    assert outcome.untracked_code == "autonomous_host"
    assert isinstance(outcome.admission, DispatchResult)
    assert len(stand_in.calls("mobkit/dispatch")) == 1
    assert stand_in.calls("mobkit/wait_for_completion")
    assert stand_in.calls("mobkit/wait_for_turn") == []


@pytest.mark.asyncio
@pytest.mark.timeout(60)
async def test_a_missing_ticket_on_a_trackable_member_raises_tracking_unavailable(tmp_path):
    stand_in = _StandIn(tmp_path, {
        "mobkit/dispatch": [{"result": _admitted(None, unavailable="runtime_refused")}],
        **_PEER_COMPLETED,
    })
    runtime = await _runtime(stand_in)
    try:
        with pytest.raises(TurnTrackingUnavailableError) as raised:
            await runtime.agent(_IDENTITY).dispatch_text_and_wait(
                "process the incident", timeout=5,
            )
    finally:
        await runtime.shutdown()

    error = raised.value
    assert isinstance(error.admission, DispatchResult)
    assert error.admission.turn_ticket is None
    assert (error.code, error.reason) == ("runtime_refused", "because runtime_refused")
    assert len(stand_in.calls("mobkit/dispatch")) == 1
    assert not [r for r in stand_in.requests() if r["method"] in _IDENTITY_WIDE]
    assert stand_in.calls("mobkit/wait_for_turn") == []


@pytest.mark.asyncio
@pytest.mark.timeout(60)
async def test_require_attribution_raises_on_the_default_mode(tmp_path):
    stand_in = _StandIn(tmp_path, {
        "mobkit/dispatch": [{"result": _admitted(None, unavailable="autonomous_host")}],
        **_PEER_COMPLETED,
    })
    runtime = await _runtime(stand_in)
    try:
        with pytest.raises(TurnTrackingUnavailableError) as raised:
            await runtime.agent(_IDENTITY).dispatch_text_and_wait(
                "process the incident", timeout=5, require_attribution=True,
            )
    finally:
        await runtime.shutdown()

    assert raised.value.code == "autonomous_host"
    assert isinstance(raised.value.admission, DispatchResult)
    assert len(stand_in.calls("mobkit/dispatch")) == 1
    assert not [r for r in stand_in.requests() if r["method"] in _IDENTITY_WIDE]


@pytest.mark.asyncio
@pytest.mark.timeout(60)
async def test_a_trackable_member_can_opt_into_the_non_attributed_wait(tmp_path):
    stand_in = _StandIn(tmp_path, {
        "mobkit/dispatch": [{"result": _admitted(None, unavailable="runtime_refused")}],
        **_PEER_COMPLETED,
    })
    runtime = await _runtime(stand_in)
    try:
        with pytest.warns(TurnTrackingUnavailableWarning, match="runtime_refused"):
            output = await runtime.agent(_IDENTITY).dispatch_text_and_wait(
                "process the incident", timeout=5, allow_identity_wide_fallback=True,
            )
    finally:
        await runtime.shutdown()

    assert output == "peer reply"
    assert len(stand_in.calls("mobkit/dispatch")) == 1


@pytest.mark.asyncio
@pytest.mark.timeout(60)
async def test_cancelling_the_wait_never_redispatches(tmp_path):
    stand_in = _StandIn(tmp_path, {
        "mobkit/dispatch": [{"result": _admitted(_TICKET)}],
        "mobkit/wait_for_turn": [{"sleep": 30, **_own("too late")}],
    })
    runtime = await _runtime(stand_in)
    try:
        task = asyncio.create_task(
            runtime.agent(_IDENTITY).dispatch_text_and_wait("process", timeout=20)
        )
        for _ in range(200):
            if stand_in.calls("mobkit/wait_for_turn"):
                break
            await asyncio.sleep(0.01)
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
        await asyncio.sleep(0.3)
        assert len(stand_in.calls("mobkit/dispatch")) == 1
        assert len(stand_in.calls("mobkit/wait_for_turn")) == 1
    finally:
        await runtime.shutdown()


@pytest.mark.asyncio
@pytest.mark.timeout(60)
async def test_an_unknown_ticket_after_a_gateway_restart_raises_typed_with_admission(tmp_path):
    """The gateway dies after admitting the dispatch; the transport restarts
    it, and the replacement knows no such ticket. The SDK reports that,
    typed, with the admission, and does not dispatch again."""
    stand_in = _StandIn(
        tmp_path,
        first={
            "mobkit/dispatch": [{"result": _admitted(_TICKET)}],
            "mobkit/wait_for_turn": [{"crash": True}],
        },
        after_restart={
            "mobkit/wait_for_turn": [{"result": {"state": "unknown", "wait": "settled"}}],
        },
    )
    runtime = await _runtime(stand_in)
    try:
        with pytest.raises(TurnUnknownError) as raised:
            await runtime.agent(_IDENTITY).dispatch_text_and_wait("process", timeout=20)
    finally:
        await runtime.shutdown()

    assert isinstance(raised.value.admission, DispatchResult)
    assert raised.value.admission.turn_ticket == _TICKET
    assert raised.value.ticket == _TICKET
    assert len(stand_in.calls("mobkit/dispatch")) == 1
    generations = [r["generation"] for r in stand_in.calls("mobkit/wait_for_turn")]
    assert generations[0] == 1 and generations[-1] == 2
    assert not [r for r in stand_in.requests() if r["method"] in _IDENTITY_WIDE]


@pytest.mark.asyncio
@pytest.mark.timeout(60)
async def test_a_turn_pending_at_the_deadline_is_a_timeout_carrying_the_admission(tmp_path):
    stand_in = _StandIn(tmp_path, {
        "mobkit/dispatch": [{"result": _admitted(_TICKET)}],
        "mobkit/wait_for_turn": [_PENDING_AT_DEADLINE],
    })
    runtime = await _runtime(stand_in)
    try:
        with pytest.raises(TimeoutError) as raised:
            await runtime.agent(_IDENTITY).dispatch_text_and_wait("process", timeout=0.5)
    finally:
        await runtime.shutdown()

    assert isinstance(raised.value, TurnWaitTimeoutError)
    assert raised.value.admission.turn_ticket == _TICKET
    assert len(stand_in.calls("mobkit/dispatch")) == 1
