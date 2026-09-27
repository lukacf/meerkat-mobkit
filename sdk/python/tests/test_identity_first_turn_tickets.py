"""Per-admission completion for the Python SDK.

The defect: ``send_and_wait`` waited until the identity-wide completion cursor
passed the send's baseline, then returned the session's latest
``output_preview``. A concurrent delivery (a peer message, a scheduled turn,
a fork completion wake) completing first satisfied the wait, and the caller
got someone else's output.

``send_and_wait`` / ``dispatch*_and_wait`` now send with ``track_turn`` and
wait on the returned ticket through ``mobkit/turn_result``, which reports THAT
turn and its own output.
"""
import asyncio
import warnings

import pytest

from meerkat_mobkit.errors import (
    TurnFailedError,
    TurnOutputTruncatedWarning,
    TurnOutputUnavailableError,
    TurnOutputUnavailableWarning,
    TurnTrackingUnavailableWarning,
    TurnUnknownError,
)
from meerkat_mobkit.identity_first_models import (
    CompletionCursor,
    DispatchInput,
    DispatchResult,
    SendResult,
    TurnOutputStatus,
    TurnResult,
    TurnState,
    TurnUnavailable,
)
from meerkat_mobkit.runtime import IdentityAgentHandle, MobKitRuntime


class TicketTransport:
    """Answers each RPC from a script.

    ``sends`` is consumed one entry per ``mobkit/send`` (or dispatch) call;
    ``turn_results`` maps a ticket to the states ``mobkit/turn_result`` walks
    through, one per poll, holding the last. ``inspections`` does the same for
    ``mobkit/inspect_identity``: it models the identity-wide cursor moving for
    someone else's turn.
    """

    def __init__(self, *, sends=None, turn_results=None, inspections=None):
        self.calls: list[dict] = []
        self.request_timeout = 60.0
        self._sends = list(sends or [])
        self._turn_results = {k: list(v) for k, v in (turn_results or {}).items()}
        self._turn_polls: dict[str, int] = {}
        self._inspections = list(inspections or [])
        self._inspect_index = 0

    def send_sync(self, request):
        self.calls.append(request)
        method = request.get("method")
        params = request.get("params") or {}
        if method in ("mobkit/send", "mobkit/dispatch"):
            result = self._sends.pop(0)
        elif method == "mobkit/turn_result":
            ticket = params["ticket"]
            script = self._turn_results.get(ticket, [{"state": "unknown"}])
            index = min(self._turn_polls.get(ticket, 0), len(script) - 1)
            self._turn_polls[ticket] = self._turn_polls.get(ticket, 0) + 1
            result = {"identity": params["identity"], "ticket": ticket, **script[index]}
        elif method == "mobkit/inspect_identity":
            index = min(self._inspect_index, len(self._inspections) - 1)
            result = self._inspections[index]
            self._inspect_index += 1
        else:
            result = {}
        return {"jsonrpc": "2.0", "id": request.get("id"), "result": result}

    async def send_async(self, request, *, timeout=None):
        await asyncio.sleep(0)
        return self.send_sync(request)

    def is_running(self):
        return True

    def start(self):
        pass

    def stop(self):
        pass

    def set_callback_handler(self, handler):
        pass

    def params_of(self, method: str) -> list[dict]:
        return [call.get("params") or {} for call in self.calls if call["method"] == method]


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


def _sent(ticket: str | None, *, turns: int = 0, unavailable: str | None = None) -> dict:
    result = {"fencing_token": 3, "completion_baseline": {"epoch": 3, "turns": turns}}
    if ticket is not None:
        result["turn"] = {"ticket": ticket}
    elif unavailable is not None:
        result["turn"] = None
        result["turn_unavailable"] = {"code": unavailable, "reason": f"because {unavailable}"}
    return result


def _completed(output: str | None, *, status: str | None = None, truncated: bool = False) -> dict:
    return {
        "state": "completed",
        "output_status": status or ("text" if output is not None else "empty"),
        "output": output,
        "output_truncated": truncated,
    }


def _inspection(preview: str | None, turns: int) -> dict:
    return {
        "identity": "keeper",
        "output_preview": preview,
        "is_final": False,
        "peer_reachable_count": 0,
        "completion_cursor": {"epoch": 3, "turns": turns},
    }


PENDING = {"state": "pending"}


class TestSendAndWaitWaitsForItsOwnTurn:
    @pytest.mark.asyncio
    async def test_a_foreign_completion_does_not_satisfy_the_wait(self):
        """THE defect: the identity's cursor moves past the baseline for a
        foreign turn while this send's turn is still running. The wait must
        return this turn's output, not the foreign preview."""
        transport = TicketTransport(
            sends=[_sent("ticket-a", turns=0)],
            turn_results={"ticket-a": [PENDING, PENDING, _completed("A's reply")]},
            # The identity-wide cursor has already moved for a foreign turn.
            inspections=[_inspection("foreign reply", turns=1)],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        output = await handle.send_and_wait("alpha", timeout=5, poll_interval=0.001)

        assert output == "A's reply"
        assert transport.params_of("mobkit/send")[0]["track_turn"] is True
        assert transport.params_of("mobkit/inspect_identity") == [], (
            "a ticketed wait must not read the identity-wide cursor at all"
        )
        assert len(transport.params_of("mobkit/turn_result")) == 3

    @pytest.mark.asyncio
    async def test_concurrent_sends_each_get_their_own_output(self):
        transport = TicketTransport(
            sends=[_sent("ticket-a"), _sent("ticket-b")],
            turn_results={
                "ticket-a": [PENDING, PENDING, PENDING, _completed("A's reply")],
                "ticket-b": [PENDING, _completed("B's reply")],
            },
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        outputs = await asyncio.gather(
            handle.send_and_wait("alpha", timeout=5, poll_interval=0.001),
            handle.send_and_wait("beta", timeout=5, poll_interval=0.001),
        )

        assert outputs == ["A's reply", "B's reply"]

    @pytest.mark.asyncio
    async def test_the_old_cursor_wait_returns_the_foreign_preview(self):
        """Control: the identity-wide wait on the same data returns the
        foreign turn's preview. If this ever stops holding, the test above is
        passing for free."""
        transport = TicketTransport(inspections=[_inspection("foreign reply", turns=1)])
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        output = await handle.wait_for_completion(
            CompletionCursor(epoch=3, turns=0), timeout=5, poll_interval=0.001,
        )

        assert output == "foreign reply"

    @pytest.mark.asyncio
    async def test_dispatch_and_wait_waits_on_its_ticket(self):
        transport = TicketTransport(
            sends=[
                {**_sent("ticket-d"), "durable": True},
                {**_sent("ticket-t"), "durable": True},
            ],
            turn_results={
                "ticket-d": [_completed("dispatched reply")],
                "ticket-t": [_completed("text reply")],
            },
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        dispatched = await handle.dispatch_and_wait(
            DispatchInput(content="work", origin="system"), timeout=5, poll_interval=0.001,
        )
        texted = await handle.dispatch_text_and_wait(
            "more work", correlation_id="chat-1", idempotency_key="evt-1",
            timeout=5, poll_interval=0.001,
        )

        assert (dispatched, texted) == ("dispatched reply", "text reply")
        params = transport.params_of("mobkit/dispatch")
        assert all(p["track_turn"] is True for p in params)
        # The text helper carries the caller's dedup pair AND tracks the turn.
        assert params[1]["dispatch_input"]["idempotency_key"] == "evt-1"
        assert params[1]["dispatch_input"]["correlation_id"] == "chat-1"


class TestWaitForTurn:
    @pytest.mark.asyncio
    async def test_a_failed_turn_raises_typed(self):
        transport = TicketTransport(
            turn_results={"ticket-a": [{"state": "failed", "error": "the model refused"}]},
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        with pytest.raises(TurnFailedError) as raised:
            await handle.wait_for_turn("ticket-a", timeout=5, poll_interval=0.001)
        assert raised.value.reason == "the model refused"
        assert raised.value.ticket == "ticket-a"

    @pytest.mark.asyncio
    async def test_an_unknown_ticket_raises_typed_and_never_guesses(self):
        transport = TicketTransport(turn_results={"ticket-a": [{"state": "unknown"}]})
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        with pytest.raises(TurnUnknownError):
            await handle.wait_for_turn("ticket-a", timeout=5, poll_interval=0.001)

    @pytest.mark.asyncio
    async def test_a_pending_turn_times_out(self):
        transport = TicketTransport(turn_results={"ticket-a": [PENDING]})
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        with pytest.raises(TimeoutError):
            await handle.wait_for_turn("ticket-a", timeout=0.02, poll_interval=0.001)

    @pytest.mark.asyncio
    async def test_the_result_is_typed(self):
        transport = TicketTransport(turn_results={
            "ticket-a": [_completed("A's reply")],
            "ticket-e": [_completed(None)],
            "ticket-n": [_completed(None, status="no_own_result")],
            "ticket-t": [_completed("long[truncated]", truncated=True)],
        })
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        text = await handle.wait_for_turn("ticket-a", timeout=5, poll_interval=0.001)
        assert (text.output_status, text.output) == (TurnOutputStatus.TEXT, "A's reply")
        empty = await handle.wait_for_turn("ticket-e", timeout=5, poll_interval=0.001)
        assert (empty.output_status, empty.output) == (TurnOutputStatus.EMPTY, None)
        folded = await handle.wait_for_turn("ticket-n", timeout=5, poll_interval=0.001)
        assert folded.output_status is TurnOutputStatus.NO_OWN_RESULT
        cut = await handle.wait_for_turn("ticket-t", timeout=5, poll_interval=0.001)
        assert cut.output_truncated is True

    @pytest.mark.asyncio
    async def test_send_and_wait_never_returns_partial_or_absent_output_silently(self):
        transport = TicketTransport(
            sends=[_sent("ticket-t"), _sent("ticket-n"), _sent("ticket-u"), _sent("ticket-e")],
            turn_results={
                "ticket-t": [_completed("long[truncated]", truncated=True)],
                "ticket-n": [_completed(None, status="no_own_result")],
                "ticket-u": [_completed(None, status="unavailable")],
                "ticket-e": [_completed(None)],
            },
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        with pytest.warns(TurnOutputTruncatedWarning):
            assert await handle.send_and_wait("t", timeout=5, poll_interval=0.001) == (
                "long[truncated]"
            )
        with pytest.warns(TurnOutputUnavailableWarning, match="no_own_result"):
            assert await handle.send_and_wait("n", timeout=5, poll_interval=0.001) is None
        with pytest.warns(TurnOutputUnavailableWarning, match="unavailable"):
            assert await handle.send_and_wait("u", timeout=5, poll_interval=0.001) is None
        with warnings.catch_warnings():
            warnings.simplefilter("error")
            # The turn committed no text: None, and nothing to warn about.
            assert await handle.send_and_wait("e", timeout=5, poll_interval=0.001) is None

    @pytest.mark.asyncio
    async def test_wait_for_output_by_turn_returns_that_turns_output(self):
        transport = TicketTransport(
            turn_results={
                "ticket-a": [PENDING, _completed("A's reply")],
                "ticket-e": [_completed(None)],
                "ticket-n": [_completed(None, status="no_own_result")],
            },
            inspections=[_inspection("foreign reply", turns=5)],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        assert await handle.wait_for_output(
            turn="ticket-a", timeout=5, poll_interval=0.001,
        ) == "A's reply"
        with pytest.raises(TurnOutputUnavailableError) as raised:
            await handle.wait_for_output(turn="ticket-e", timeout=5, poll_interval=0.001)
        assert raised.value.status == "empty"
        with pytest.raises(TurnOutputUnavailableError) as raised:
            await handle.wait_for_output(turn="ticket-n", timeout=5, poll_interval=0.001)
        assert raised.value.status == "no_own_result"
        with pytest.raises(ValueError, match="alone"):
            await handle.wait_for_output(
                turn="ticket-a", after=CompletionCursor(epoch=3, turns=0),
            )


class TestExplicitFallback:
    @pytest.mark.asyncio
    async def test_an_old_gateway_falls_back_to_the_cursor_with_a_warning(self):
        """A gateway that predates turn tickets returns no ``turn``: the wait
        falls back to the identity-wide cursor, and says so."""
        transport = TicketTransport(
            sends=[_sent(None, turns=0)],
            inspections=[_inspection("latest reply", turns=1)],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        with pytest.warns(TurnTrackingUnavailableWarning, match="predates turn tickets"):
            output = await handle.send_and_wait("alpha", timeout=5, poll_interval=0.001)

        assert output == "latest reply"
        assert transport.params_of("mobkit/turn_result") == []

    @pytest.mark.asyncio
    async def test_an_untrackable_turn_names_the_reason(self):
        transport = TicketTransport(
            sends=[_sent(None, turns=0, unavailable="autonomous_host")],
            inspections=[_inspection("latest reply", turns=1)],
        )
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        with pytest.warns(TurnTrackingUnavailableWarning, match="autonomous_host"):
            output = await handle.send_and_wait("alpha", timeout=5, poll_interval=0.001)
        assert output == "latest reply"
        assert len(transport.params_of("mobkit/send")) == 1, "delivered exactly once"

    @pytest.mark.asyncio
    async def test_an_undelivered_send_raises_instead_of_waiting(self):
        sent = _sent(None, turns=0)
        sent["turn"] = None
        sent["turn_unavailable"] = {
            "code": "not_delivered", "reason": "no session bridge", "delivered": False,
        }
        transport = TicketTransport(sends=[sent])
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        with pytest.raises(RuntimeError, match="was not delivered"):
            await handle.send_and_wait("alpha", timeout=5, poll_interval=0.001)
        assert transport.params_of("mobkit/inspect_identity") == []

    @pytest.mark.asyncio
    async def test_plain_send_does_not_request_tracking(self):
        transport = TicketTransport(sends=[_sent(None)])
        handle = IdentityAgentHandle(_make_runtime(transport), "keeper")

        result = await handle.send("alpha")

        assert "track_turn" not in transport.params_of("mobkit/send")[0]
        assert result.turn_ticket is None


class TestModels:
    def test_results_carry_the_ticket_and_round_trip(self):
        sent = SendResult.from_dict(_sent("ticket-a", turns=2))
        assert sent.turn_ticket == "ticket-a"
        assert SendResult.from_dict(sent.to_dict()) == sent
        unavailable = DispatchResult.from_dict(
            {**_sent(None, unavailable="no bridge"), "durable": False}
        )
        assert unavailable.turn_ticket is None
        assert unavailable.turn_unavailable == TurnUnavailable(
            code="no bridge", reason="because no bridge", delivered=True
        )
        assert DispatchResult.from_dict(unavailable.to_dict()) == unavailable
        # Older payloads without the flag: only not_delivered means undelivered.
        assert TurnUnavailable.from_wire({"code": "not_delivered", "reason": "x"}).delivered is False
        assert TurnUnavailable.from_wire({"code": "autonomous_host", "reason": "x"}).delivered is True

    def test_turn_result_parses_every_state(self):
        completed = TurnResult.from_dict(
            {"identity": "keeper", "ticket": "t", **_completed("hi"),
             "completion_cursor": {"epoch": 3, "turns": 4}}
        )
        assert completed.state is TurnState.COMPLETED
        assert completed.output_status is TurnOutputStatus.TEXT
        assert completed.output == "hi"
        assert completed.completion_cursor == CompletionCursor(epoch=3, turns=4)
        assert TurnResult.from_dict(completed.to_dict()) == completed
        failed = TurnResult.from_dict({"identity": "keeper", "ticket": "t",
                                       "state": "failed", "error": "boom"})
        assert (failed.state, failed.error) == (TurnState.FAILED, "boom")
        assert TurnResult.from_dict({"state": "surprise"}).state is TurnState.UNKNOWN
        future = TurnResult.from_dict({"state": "completed", "output_status": "later", "output": "x"})
        assert future.output_status is TurnOutputStatus.UNAVAILABLE
        assert future.output is None, "text is only read for the text status"
