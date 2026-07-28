"""Turn-completion contract for the Python SDK.

The defect these tests pin: `wait_for_output(baseline=<text>)` waits for the
output text to CHANGE. Two consecutive turns that both answer exactly `ACK`
are indistinguishable from no turn at all, so the call sleeps out its entire
timeout — a 900s wait reported as a "962-second turn" that never happened.

The replacement waits on a cursor: `{epoch, turns}`, where `epoch` is the
identity's lease incarnation and `turns` counts completed turns within it.
"""
import asyncio
import warnings

import pytest

from meerkat_mobkit.identity_first_models import (
    CompletionCursor,
    CompletionProgress,
    DispatchInput,
    DispatchResult,
    IdentityInspection,
    SendResult,
)
from meerkat_mobkit.runtime import IdentityAgentHandle, MobKitRuntime


class ScriptedTransport:
    """Transport that answers each RPC method from a scripted queue.

    `inspect_identity` walks its script one entry per poll, holding the last
    entry once exhausted, so a test can model "still running, still running,
    done" without racing a clock.
    """

    def __init__(self, *, send=None, dispatch=None, inspections=None):
        self.calls: list[dict] = []
        self.request_timeout = 60.0
        self._send = send or {}
        self._dispatch = dispatch or {}
        self._inspections = list(inspections or [])
        self._inspect_index = 0

    def send_sync(self, request):
        self.calls.append(request)
        method = request.get("method")
        if method == "mobkit/send":
            result = self._send
        elif method == "mobkit/dispatch":
            result = self._dispatch
        elif method == "mobkit/inspect_identity":
            index = min(self._inspect_index, len(self._inspections) - 1)
            result = self._inspections[index]
            self._inspect_index += 1
        else:
            result = {}
        return {"jsonrpc": "2.0", "id": request.get("id"), "result": result}

    async def send_async(self, request, *, timeout=None):
        return self.send_sync(request)

    def is_running(self):
        return True

    def start(self):
        pass

    def stop(self):
        pass

    def set_callback_handler(self, handler):
        pass

    @property
    def inspect_calls(self) -> int:
        return sum(
            1 for call in self.calls if call.get("method") == "mobkit/inspect_identity"
        )


def _make_runtime(transport) -> MobKitRuntime:
    rt = MobKitRuntime.__new__(MobKitRuntime)
    rt._config = None
    rt._transport = transport
    rt._running = True
    rt._rust_http_base = None
    rt._lifecycle_lock = asyncio.Lock()
    rt._shutdown_task = None
    from meerkat_mobkit.agent_builder import CallbackDispatcher

    rt._dispatcher = CallbackDispatcher()
    return rt


def _inspection(identity: str, preview: str | None, epoch: int, turns: int) -> dict:
    return {
        "identity": identity,
        "output_preview": preview,
        "is_final": False,
        "peer_reachable_count": 0,
        "completion_cursor": {"epoch": epoch, "turns": turns},
    }


# ---------------------------------------------------------------------------
# The production regression
# ---------------------------------------------------------------------------


class TestIdenticalConsecutiveOutput:
    @pytest.mark.asyncio
    async def test_second_identical_ack_turn_is_detected(self):
        """THE defect: two turns both answering exactly `ACK`.

        The second completion must be detected. The old text comparison could
        not see it at all.
        """
        transport = ScriptedTransport(
            send={
                "fencing_token": 7,
                "completion_baseline": {"epoch": 7, "turns": 1},
            },
            inspections=[
                # Turn 2 in flight — the PREVIOUS turn's `ACK` is still the
                # visible output.
                _inspection("triage:main", "ACK", epoch=7, turns=1),
                # Turn 2 committed. Same text, byte for byte.
                _inspection("triage:main", "ACK", epoch=7, turns=2),
            ],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "triage:main")

        output = await handle.send_and_wait("ping", timeout=5, poll_interval=0.01)

        assert output == "ACK"
        assert transport.inspect_calls == 2, (
            "the waiter must have polled past the first (unchanged) inspection "
            "instead of returning on it"
        )

    @pytest.mark.asyncio
    async def test_text_baseline_path_cannot_see_the_identical_turn(self):
        """The deprecated path, on the same data, times out. Kept as the proof
        that the scenario really is the one that defeats a text comparison —
        if this ever passes, the cursor test above is passing for free."""
        transport = ScriptedTransport(
            inspections=[_inspection("triage:main", "ACK", epoch=7, turns=2)],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "triage:main")

        with warnings.catch_warnings():
            warnings.simplefilter("ignore", DeprecationWarning)
            with pytest.raises(TimeoutError):
                await handle.wait_for_output(
                    timeout=0.05, poll_interval=0.01, baseline="ACK"
                )

    @pytest.mark.asyncio
    async def test_text_baseline_path_warns_as_deprecated(self):
        transport = ScriptedTransport(
            inspections=[_inspection("triage:main", "ACK", epoch=7, turns=2)],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "triage:main")

        with pytest.warns(DeprecationWarning, match="unsound"):
            with pytest.raises(TimeoutError):
                await handle.wait_for_output(
                    timeout=0.05, poll_interval=0.01, baseline="ACK"
                )

    @pytest.mark.asyncio
    async def test_wait_for_output_after_cursor_sees_identical_turn(self):
        """The retained `wait_for_output` entry point, driven by `after`."""
        transport = ScriptedTransport(
            inspections=[
                _inspection("triage:main", "ACK", epoch=7, turns=1),
                _inspection("triage:main", "ACK", epoch=7, turns=2),
            ],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "triage:main")

        output = await handle.wait_for_output(
            timeout=5,
            poll_interval=0.01,
            after=CompletionCursor(epoch=7, turns=1),
        )

        assert output == "ACK"
        assert transport.inspect_calls == 2

    def test_after_and_baseline_are_mutually_exclusive(self):
        transport = ScriptedTransport(inspections=[_inspection("a", None, 1, 0)])
        handle = IdentityAgentHandle(_make_runtime(transport), "a")

        with pytest.raises(ValueError, match="not both"):
            asyncio.run(
                handle.wait_for_output(
                    after=CompletionCursor(epoch=1, turns=0), baseline="ACK"
                )
            )


# ---------------------------------------------------------------------------
# Waiter semantics
# ---------------------------------------------------------------------------


class TestWaitForCompletion:
    @pytest.mark.asyncio
    async def test_monotonic_cursor_across_several_turns(self):
        transport = ScriptedTransport(
            inspections=[
                _inspection("triage:main", "ACK", epoch=3, turns=turns)
                for turns in (0, 1, 2, 3, 4)
            ],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "triage:main")

        seen = []
        for _ in range(5):
            inspection = await handle.inspect()
            seen.append(inspection.completion_cursor.turns)

        assert seen == [0, 1, 2, 3, 4]
        assert all(b > a for a, b in zip(seen, seen[1:])), "strictly increasing"

    @pytest.mark.asyncio
    async def test_stalled_turn_times_out(self):
        transport = ScriptedTransport(
            inspections=[_inspection("triage:main", "ACK", epoch=3, turns=1)],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "triage:main")

        with pytest.raises(TimeoutError, match="did not complete a turn"):
            await handle.wait_for_completion(
                CompletionCursor(epoch=3, turns=1), timeout=0.05, poll_interval=0.01
            )

    @pytest.mark.asyncio
    async def test_incarnation_change_is_reported_not_guessed(self):
        transport = ScriptedTransport(
            inspections=[_inspection("triage:main", "ACK", epoch=9, turns=0)],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "triage:main")

        with pytest.raises(RuntimeError, match="superseded runtime incarnation"):
            await handle.wait_for_completion(
                CompletionCursor(epoch=3, turns=1), timeout=5, poll_interval=0.01
            )

    @pytest.mark.asyncio
    async def test_missing_cursor_raises_instead_of_falling_back_to_text(self):
        """An older gateway must fail loudly, not silently reintroduce the bug."""
        transport = ScriptedTransport(
            send={"fencing_token": 7},
            inspections=[
                {
                    "identity": "triage:main",
                    "output_preview": "ACK",
                    "is_final": False,
                    "peer_reachable_count": 0,
                }
            ],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "triage:main")

        with pytest.raises(RuntimeError, match="no completion_baseline"):
            await handle.send_and_wait("ping", timeout=1, poll_interval=0.01)

    @pytest.mark.asyncio
    async def test_dispatch_and_wait_threads_its_own_baseline(self):
        transport = ScriptedTransport(
            dispatch={
                "fencing_token": 4,
                "durable": True,
                "completion_baseline": {"epoch": 4, "turns": 5},
            },
            inspections=[
                _inspection("internal:main", "ACK", epoch=4, turns=5),
                _inspection("internal:main", "ACK", epoch=4, turns=6),
            ],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "internal:main")

        output = await handle.dispatch_and_wait(
            DispatchInput(content="go", origin="system"), timeout=5, poll_interval=0.01
        )

        assert output == "ACK"
        assert transport.inspect_calls == 2


# ---------------------------------------------------------------------------
# Cursor value semantics
# ---------------------------------------------------------------------------


class TestCompletionCursor:
    def test_progress_classification(self):
        baseline = CompletionCursor(epoch=2, turns=3)
        assert (
            baseline.progress_since(baseline) is CompletionProgress.PENDING
        ), "an unchanged cursor is Pending regardless of what the agent said"
        assert (
            CompletionCursor(epoch=2, turns=4).progress_since(baseline)
            is CompletionProgress.COMPLETED
        )
        assert (
            CompletionCursor(epoch=3, turns=0).progress_since(baseline)
            is CompletionProgress.INCARNATION_CHANGED
        )

    def test_round_trip(self):
        cursor = CompletionCursor(epoch=12, turns=34)
        assert cursor.to_dict() == {"epoch": 12, "turns": 34}
        assert CompletionCursor.from_dict(cursor.to_dict()) == cursor


# ---------------------------------------------------------------------------
# Wire mirrors: both directions, both fields
# ---------------------------------------------------------------------------


class TestModelMirrors:
    def test_identity_inspection_carries_cursor_both_ways(self):
        payload = _inspection("triage:main", "ACK", epoch=7, turns=2)

        parsed = IdentityInspection.from_dict(payload)
        assert parsed.completion_cursor == CompletionCursor(epoch=7, turns=2)
        assert parsed.to_dict()["completion_cursor"] == {"epoch": 7, "turns": 2}
        assert IdentityInspection.from_dict(parsed.to_dict()) == parsed

    def test_dispatch_result_carries_baseline_both_ways(self):
        payload = {
            "fencing_token": 4,
            "durable": True,
            "completion_baseline": {"epoch": 4, "turns": 5},
        }

        parsed = DispatchResult.from_dict(payload)
        assert parsed.completion_baseline == CompletionCursor(epoch=4, turns=5)
        assert parsed.to_dict() == payload
        assert DispatchResult.from_dict(parsed.to_dict()) == parsed

    def test_send_result_carries_baseline_both_ways(self):
        payload = {"fencing_token": 4, "completion_baseline": {"epoch": 4, "turns": 5}}

        parsed = SendResult.from_dict(payload)
        assert parsed.completion_baseline == CompletionCursor(epoch=4, turns=5)
        assert parsed.to_dict() == payload
        assert SendResult.from_dict(parsed.to_dict()) == parsed

    def test_payloads_without_the_field_still_deserialize(self):
        """Backward compatibility: an older gateway's payloads must parse, and
        absence must read as `None` — never as a fabricated zero cursor, which
        a caller could mistake for 'no turns yet'."""
        inspection = IdentityInspection.from_dict(
            {"identity": "triage:main", "output_preview": "ACK", "is_final": True}
        )
        assert inspection.completion_cursor is None
        assert inspection.output_preview == "ACK"
        assert inspection.is_final is True
        assert "completion_cursor" not in inspection.to_dict()

        dispatch = DispatchResult.from_dict({"fencing_token": 2, "durable": False})
        assert dispatch.completion_baseline is None
        assert dispatch.fencing_token == 2

        send = SendResult.from_dict({"fencing_token": 2})
        assert send.completion_baseline is None
        assert send.fencing_token == 2

    def test_null_cursor_reads_as_absent(self):
        """Live aliases report `completion_cursor: null` — not tracked, which
        is not the same as zero turns."""
        inspection = IdentityInspection.from_dict(
            {"identity": "live:alias", "completion_cursor": None}
        )
        assert inspection.completion_cursor is None


class TestDispatchTextConvenience:
    @pytest.mark.asyncio
    async def test_dispatch_text_and_wait_threads_its_own_baseline(self):
        transport = ScriptedTransport(
            dispatch={
                "fencing_token": 4,
                "durable": True,
                "completion_baseline": {"epoch": 4, "turns": 0},
            },
            inspections=[
                _inspection("triage:main", None, epoch=4, turns=0),
                _inspection("triage:main", "ACK", epoch=4, turns=1),
            ],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "triage:main")

        output = await handle.dispatch_text_and_wait(
            "New event", origin="connector", timeout=5, poll_interval=0.01
        )

        assert output == "ACK"
        dispatched = next(
            call for call in transport.calls if call["method"] == "mobkit/dispatch"
        )
        assert dispatched["params"]["dispatch_input"]["origin"] == "connector"


class TestPerIdentityCorrelation:
    @pytest.mark.asyncio
    async def test_another_identitys_completion_does_not_satisfy_the_wait(self):
        """Dispatch A's wait is not satisfied by dispatch B's completion.

        The cursor is per-identity, so the waiter must only ever read its own
        identity's cursor — even while a busy neighbour is completing turns.
        """
        transport = ScriptedTransport(
            dispatch={
                "fencing_token": 4,
                "durable": True,
                "completion_baseline": {"epoch": 4, "turns": 2},
            },
            # This identity's cursor never moves, whatever anyone else does.
            inspections=[_inspection("triage:main", "ACK", epoch=4, turns=2)],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "triage:main")

        with pytest.raises(TimeoutError):
            await handle.dispatch_and_wait(
                DispatchInput(content="A", origin="system"),
                timeout=0.05,
                poll_interval=0.01,
            )

        polled = {
            call["params"]["identity"]
            for call in transport.calls
            if call["method"] == "mobkit/inspect_identity"
        }
        assert polled == {"triage:main"}, (
            f"the waiter must poll only its own identity, polled {polled}"
        )

    @pytest.mark.asyncio
    async def test_two_identities_carry_independent_cursors(self):
        """Turns on one identity never advance another's cursor."""
        busy = ScriptedTransport(
            inspections=[_inspection("worker:alpha", "done", epoch=9, turns=7)],
        )
        quiet = ScriptedTransport(
            inspections=[_inspection("triage:main", "ACK", epoch=4, turns=1)],
        )

        busy_cursor = (
            await IdentityAgentHandle(_make_runtime(busy), "worker:alpha").inspect()
        ).completion_cursor
        quiet_cursor = (
            await IdentityAgentHandle(_make_runtime(quiet), "triage:main").inspect()
        ).completion_cursor

        assert busy_cursor == CompletionCursor(epoch=9, turns=7)
        assert quiet_cursor == CompletionCursor(epoch=4, turns=1)
        assert (
            quiet_cursor.progress_since(CompletionCursor(epoch=4, turns=1))
            is CompletionProgress.PENDING
        ), "the busy neighbour's 7 turns must not register as progress here"
