"""Transport robustness (#550, part 1).

One test per defect:
- a bad stdout line (undecodable bytes, JSON that is not an object) must be
  skipped, not end the reader thread;
- any reader exit (EOF or a reader exception) must fail every waiter with the
  typed ``TransportReaderFailedError`` and fail later sends immediately;
- a callback result with a non-finite number must answer the gateway with a
  typed error, and no line with a ``NaN``/``Infinity`` token is ever written;
- a callback request with no registered handler must be answered at once with
  a typed error instead of leaving the gateway waiting.
"""

from __future__ import annotations

import json
import threading
from unittest.mock import AsyncMock, MagicMock

import pytest

from meerkat_mobkit import TransportError, TransportReaderFailedError
from meerkat_mobkit._transport import PersistentTransport


def _reader_transport(lines: list[bytes]) -> PersistentTransport:
    transport = PersistentTransport("unused")
    process = MagicMock()
    process.stdout.readline.side_effect = lines
    transport._process = process
    return transport


def test_reader_skips_undecodable_and_non_object_lines():
    transport = _reader_transport([
        b"\xff\xfe not utf-8\n",
        b"5\n",
        b"[1, 2]\n",
        b"null\n",
        b'{"jsonrpc":"2.0","id":"r1","result":{"ok":true}}\n',
        b"",
    ])
    event = threading.Event()
    transport._pending["r1"] = event

    transport._reader_loop()

    assert event.is_set()
    assert transport._results["r1"] == {"jsonrpc": "2.0", "id": "r1", "result": {"ok": True}}


class _GatedStdout:
    """stdout whose first readline returns only after the request is written."""

    def __init__(self, ending: object):
        self.written = threading.Event()
        self._ending = ending

    def readline(self) -> bytes:
        self.written.wait(timeout=5)
        if isinstance(self._ending, BaseException):
            raise self._ending
        return self._ending  # type: ignore[return-value]


def _live_transport(ending: object) -> tuple[PersistentTransport, _GatedStdout]:
    transport = PersistentTransport("unused", timeout=30.0)
    stdout = _GatedStdout(ending)
    process = MagicMock()
    process.poll.return_value = None
    process.stdout = stdout
    process.stdin.write.side_effect = lambda _data: stdout.written.set()
    transport._process = process
    transport._ensure_running = lambda: None
    transport._reader_thread = threading.Thread(target=transport._reader_loop, daemon=True)
    transport._reader_thread.start()
    return transport, stdout


@pytest.mark.parametrize(
    ("ending", "reason"),
    [
        (b"", "closed its stdout"),
        (OSError("pipe read failed"), "pipe read failed"),
    ],
)
def test_reader_exit_fails_waiters_with_typed_error(ending, reason):
    transport, _stdout = _live_transport(ending)

    with pytest.raises(TransportReaderFailedError, match=reason) as raised:
        transport.send_sync({"jsonrpc": "2.0", "id": "w1", "method": "mobkit/status"})

    assert isinstance(raised.value, TransportError)
    assert reason in raised.value.reason
    transport._reader_thread.join(timeout=5)
    # The reader is gone: a later send fails at once, it does not wait out
    # its timeout for a response nobody will read.
    with pytest.raises(TransportReaderFailedError):
        transport.send_sync({"jsonrpc": "2.0", "id": "w2", "method": "mobkit/status"}, timeout=30.0)


def test_reader_exit_keeps_a_response_that_arrived_before_eof():
    # A fail-closed init writes its typed error and exits: the waiter must get
    # that error, not the reader failure.
    transport = _reader_transport([
        b'{"jsonrpc":"2.0","id":"init","error":{"code":-32014,"message":"refused"}}\n',
        b"",
    ])
    event = threading.Event()
    transport._pending["init"] = event

    transport._reader_loop()

    assert transport._results["init"]["error"]["code"] == -32014


def _run_callback(transport: PersistentTransport, result: object) -> MagicMock:
    import asyncio

    loop = asyncio.new_event_loop()
    thread = threading.Thread(target=loop.run_forever, daemon=True)
    thread.start()
    try:
        transport._loop = loop
        transport._callback_handler = AsyncMock(return_value=result)
        transport._write_line = MagicMock()
        transport._dispatch_callback(
            {"jsonrpc": "2.0", "id": "cb-7", "method": "callback/roster_provider/roster", "params": {}}
        )
        return transport._write_line
    finally:
        loop.call_soon_threadsafe(loop.stop)
        thread.join(timeout=5)
        loop.close()


@pytest.mark.parametrize("value", [float("nan"), float("inf"), float("-inf")])
def test_non_finite_callback_result_answers_a_typed_error(value):
    write = _run_callback(PersistentTransport("unused"), {"roster": [{"weight": value}]})

    response = write.call_args.args[0]
    assert response["id"] == "cb-7"
    assert "result" not in response
    assert response["error"]["data"] == {"kind": "non_finite_result"}
    assert "NaN or Infinity" in response["error"]["message"]


def test_write_line_never_emits_non_finite_tokens():
    transport = PersistentTransport("unused")
    process = MagicMock()
    transport._process = process

    with pytest.raises(ValueError):
        transport._write_line({"jsonrpc": "2.0", "id": "x", "result": float("nan")})

    process.stdin.write.assert_not_called()


def test_callback_without_handler_is_answered_at_once():
    transport = PersistentTransport("unused")
    transport._write_line = MagicMock()

    transport._handle_callback(
        {"jsonrpc": "2.0", "id": "cb-3", "method": "callback/build_agent", "params": {}}
    )

    response = transport._write_line.call_args.args[0]
    assert response["id"] == "cb-3"
    assert response["error"]["data"] == {
        "kind": "callback_handler_unavailable",
        "method": "callback/build_agent",
    }


def test_notification_without_handler_writes_nothing():
    transport = PersistentTransport("unused")
    transport._write_line = MagicMock()

    transport._handle_callback({"jsonrpc": "2.0", "method": "notification/x", "params": {}})

    transport._write_line.assert_not_called()


def test_written_callback_response_is_strict_json():
    transport = PersistentTransport("unused")
    process = MagicMock()
    transport._process = process

    transport._write_line({"jsonrpc": "2.0", "id": "cb-1", "result": {"n": 1.5}})

    line = process.stdin.write.call_args.args[0].decode("utf-8")
    assert json.loads(line, parse_constant=lambda token: pytest.fail(f"non-JSON token {token}"))


def test_callback_response_is_written_before_queued_requests():
    """A callback response waits only for the line being written, never
    behind requests queued for the pipe (#550 head-of-line)."""
    transport = PersistentTransport("unused")
    first_write_started = threading.Event()
    release_first_write = threading.Event()
    written: list[str] = []

    def write(data: bytes) -> None:
        line = json.loads(data.decode("utf-8"))
        written.append(line["id"])
        if line["id"] == "req-1":
            first_write_started.set()
            assert release_first_write.wait(timeout=5)

    process = MagicMock()
    process.stdin.write.side_effect = write
    transport._process = process

    first = threading.Thread(
        target=transport._write_line, args=({"jsonrpc": "2.0", "id": "req-1", "method": "m"},)
    )
    first.start()
    assert first_write_started.wait(timeout=5)
    # A second request queues behind the blocked write, then a callback
    # response arrives.
    second = threading.Thread(
        target=transport._write_line, args=({"jsonrpc": "2.0", "id": "req-2", "method": "m"},)
    )
    second.start()
    while not transport._write_cond._waiters:  # the second request is waiting its turn
        threading.Event().wait(0.001)
    response = threading.Thread(
        target=transport._write_line,
        args=({"jsonrpc": "2.0", "id": "cb-1", "result": {}},),
        kwargs={"callback_response": True},
    )
    response.start()
    while transport._priority_writers_waiting == 0:
        threading.Event().wait(0.001)
    release_first_write.set()
    for thread in (first, second, response):
        thread.join(timeout=5)

    assert written == ["req-1", "cb-1", "req-2"]
