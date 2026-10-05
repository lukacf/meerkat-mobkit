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

import asyncio
import json
import queue
import threading
import time
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


def _callback_process() -> tuple[MagicMock, queue.Queue[bytes], list[dict]]:
    """A child whose actual transport reader and writer exchange JSON lines."""
    lines: queue.Queue[bytes] = queue.Queue()
    writes: list[dict] = []
    process = MagicMock()
    process.poll.return_value = None
    process.stdout.readline.side_effect = lines.get
    process.stdin.write.side_effect = lambda data: writes.append(json.loads(data))

    def reap(*, timeout):
        process.poll.return_value = 0
        lines.put(b"")
        return 0

    process.wait.side_effect = reap
    return process, lines, writes


@pytest.mark.asyncio
@pytest.mark.parametrize("old_outcome", ["result", "error", "cancelled"])
@pytest.mark.parametrize("queued_reply", [False, True])
async def test_callback_completion_cannot_answer_replacement_child(monkeypatch, old_outcome, queued_reply):
    transport = PersistentTransport("unused")
    old, old_lines, old_writes = _callback_process()
    current, current_lines, current_writes = _callback_process()
    children = iter([old, current])
    monkeypatch.setattr("meerkat_mobkit._transport.subprocess.Popen", lambda *args, **kwargs: next(children))
    entered = {name: asyncio.Event() for name in ("original", "old", "current")}
    release = {name: asyncio.Event() for name in ("old", "current")}
    finished = {name: threading.Event() for name in entered}

    async def handler(_method, params):
        name = params["invocation"]
        entered[name].set()
        if name in release:
            await release[name].wait()
        if name == "old" and old_outcome == "error":
            raise ValueError("old invocation failed")
        if name == "old" and old_outcome == "cancelled":
            raise asyncio.CancelledError()
        return {"invocation": name}

    dispatch = transport._dispatch_callback

    def observe_dispatch(msg, *args, **kwargs):
        try:
            dispatch(msg, *args, **kwargs)
        finally:
            finished[msg["params"]["invocation"]].set()

    monkeypatch.setattr(transport, "_dispatch_callback", observe_dispatch)
    transport.set_callback_handler(handler)

    def send(lines, name, callback_id="cb-1"):
        lines.put(json.dumps({
            "jsonrpc": "2.0", "id": callback_id, "method": "callback/call_tool",
            "params": {"invocation": name},
        }).encode() + b"\n")

    readers = []
    blocker = None
    release_writer = threading.Event()
    try:
        transport.start()
        readers.append(transport._reader_thread)
        send(old_lines, "original", "cb-0")
        assert await asyncio.to_thread(finished["original"].wait, 2)
        assert old_writes == [{
            "jsonrpc": "2.0", "id": "cb-0", "result": {"invocation": "original"},
        }]

        send(old_lines, "old")
        await asyncio.wait_for(entered["old"].wait(), 2)
        if queued_reply:
            writer_started = threading.Event()

            def blocked_write(data):
                old_writes.append(json.loads(data))
                writer_started.set()
                assert release_writer.wait(2)

            old.stdin.write.side_effect = blocked_write
            blocker = threading.Thread(target=transport._write_line, args=({
                "jsonrpc": "2.0", "id": "old-request", "method": "mobkit/status",
            },))
            blocker.start()
            assert await asyncio.to_thread(writer_started.wait, 2)
            release["old"].set()
            deadline = time.monotonic() + 2
            while transport._priority_writers_waiting == 0:
                assert time.monotonic() < deadline
                await asyncio.sleep(0.001)
        transport.stop()
        await asyncio.to_thread(readers[0].join, 2)
        transport.start()
        readers.append(transport._reader_thread)
        send(current_lines, "current")
        await asyncio.wait_for(entered["current"].wait(), 2)

        # The replacement owns this reused callback id. Let the old handler
        # finish completely before its own handler is allowed to answer.
        release["old"].set()
        release_writer.set()
        assert await asyncio.to_thread(finished["old"].wait, 2)
        assert current_writes == [], "old completion answered the replacement's callback"
        assert len(old_writes) == 1 + int(queued_reply), "retired child received a late callback reply"

        release["current"].set()
        assert await asyncio.to_thread(finished["current"].wait, 2)
        assert current_writes == [{
            "jsonrpc": "2.0", "id": "cb-1", "result": {"invocation": "current"},
        }]
    finally:
        release_writer.set()
        for gate in release.values():
            gate.set()
        for name in entered:
            if entered[name].is_set():
                await asyncio.to_thread(finished[name].wait, 2)
        transport.stop()
        for reader in readers:
            await asyncio.to_thread(reader.join, 2)
        if blocker is not None:
            await asyncio.to_thread(blocker.join, 2)


@pytest.mark.asyncio
async def test_retired_reader_exit_cannot_fail_replacement_request(monkeypatch):
    transport = PersistentTransport("unused", timeout=2)
    old, old_lines, _old_writes = _callback_process()
    current, current_lines, current_writes = _callback_process()
    # Reaping the old process need not coincide with its reader unwinding.
    def reap_old(*, timeout):
        old.poll.return_value = 0
        return 0

    old.wait.side_effect = reap_old
    children = iter([old, current])
    monkeypatch.setattr("meerkat_mobkit._transport.subprocess.Popen", lambda *args, **kwargs: next(children))
    request_written = threading.Event()

    def current_write(data):
        current_writes.append(json.loads(data))
        request_written.set()

    current.stdin.write.side_effect = current_write
    readers = []
    pending = None
    try:
        transport.start()
        readers.append(transport._reader_thread)
        transport.stop()
        transport.start()
        readers.append(transport._reader_thread)
        pending = asyncio.create_task(transport.send_async({
            "jsonrpc": "2.0", "id": "current-request", "method": "mobkit/status",
        }))
        assert await asyncio.to_thread(request_written.wait, 2)

        old_lines.put(b"")
        await asyncio.to_thread(readers[0].join, 2)
        assert not readers[0].is_alive()
        assert transport._reader_failure is None, "retired reader failed the replacement"
        current_lines.put(b'{"jsonrpc":"2.0","id":"current-request","result":{"current":true}}\n')
        assert await pending == {
            "jsonrpc": "2.0", "id": "current-request", "result": {"current": True},
        }
        assert transport._pending == {}
    finally:
        old_lines.put(b"")
        transport.stop()
        if pending is not None:
            await asyncio.gather(pending, return_exceptions=True)
        for reader in readers:
            await asyncio.to_thread(reader.join, 2)


@pytest.mark.asyncio
async def test_retired_stop_cannot_close_replacement_stderr(monkeypatch):
    transport = PersistentTransport("unused")
    old, old_lines, _old_writes = _callback_process()
    current, _current_lines, _current_writes = _callback_process()
    old_stderr, current_stderr = MagicMock(), MagicMock()
    reaping = threading.Event()
    release = threading.Event()

    def reap_old(*, timeout):
        old.poll.return_value = 0
        reaping.set()
        assert release.wait(2)
        old_lines.put(b"")
        return 0

    old.wait.side_effect = reap_old
    children = iter([old, current])
    monkeypatch.setattr("meerkat_mobkit._transport.subprocess.Popen", lambda *args, **kwargs: next(children))
    transport.start()
    old_reader = transport._reader_thread
    transport._stderr_file = old_stderr
    stopping = asyncio.create_task(asyncio.to_thread(transport.stop))
    current_reader = None
    try:
        assert await asyncio.to_thread(reaping.wait, 2)
        transport.start()
        current_reader = transport._reader_thread
        transport._stderr_file = current_stderr
        release.set()
        await stopping
        assert transport._process is current
        assert transport._stderr_file is current_stderr
        current_stderr.close.assert_not_called()
        old_stderr.close.assert_called_once_with()
    finally:
        release.set()
        await stopping
        transport.stop()
        await asyncio.to_thread(old_reader.join, 2)
        if current_reader is not None:
            await asyncio.to_thread(current_reader.join, 2)


@pytest.mark.asyncio
async def test_callback_queued_on_loop_cannot_invoke_after_reconnect(monkeypatch):
    transport = PersistentTransport("unused")
    old, old_lines, old_writes = _callback_process()
    current, current_lines, current_writes = _callback_process()
    children = iter([old, current])
    monkeypatch.setattr("meerkat_mobkit._transport.subprocess.Popen", lambda *args, **kwargs: next(children))
    scheduled, release = asyncio.Event(), asyncio.Event()
    finished = {name: threading.Event() for name in ("old", "current")}
    effects = []
    schedule = asyncio.run_coroutine_threadsafe

    def delayed_schedule(coroutine, loop):
        async def before_invocation():
            scheduled.set()
            await release.wait()
            return await coroutine

        return schedule(before_invocation(), loop)

    monkeypatch.setattr("meerkat_mobkit._transport.asyncio.run_coroutine_threadsafe", delayed_schedule)

    async def handler(_method, params):
        effects.append(params["invocation"])
        return {"invocation": params["invocation"]}

    dispatch = transport._dispatch_callback

    def observe_dispatch(msg, *args, **kwargs):
        try:
            dispatch(msg, *args, **kwargs)
        finally:
            finished[msg["params"]["invocation"]].set()

    monkeypatch.setattr(transport, "_dispatch_callback", observe_dispatch)
    transport.set_callback_handler(handler)
    readers = []
    try:
        transport.start()
        readers.append(transport._reader_thread)
        old_lines.put(b'{"jsonrpc":"2.0","id":"cb-1","method":"callback/call_tool","params":{"invocation":"old"}}\n')
        await asyncio.wait_for(scheduled.wait(), 2)
        transport.stop()
        await asyncio.to_thread(readers[0].join, 2)
        transport.start()
        readers.append(transport._reader_thread)
        release.set()
        assert await asyncio.to_thread(finished["old"].wait, 2)
        assert effects == [], "queued old callback invoked its handler after reconnect"
        assert old_writes == []
        assert current_writes == []

        current_lines.put(b'{"jsonrpc":"2.0","id":"cb-1","method":"callback/call_tool","params":{"invocation":"current"}}\n')
        assert await asyncio.to_thread(finished["current"].wait, 2)
        assert effects == ["current"]
        assert current_writes == [{
            "jsonrpc": "2.0", "id": "cb-1", "result": {"invocation": "current"},
        }]
    finally:
        release.set()
        await asyncio.to_thread(finished["old"].wait, 2)
        transport.stop()
        for reader in readers:
            await asyncio.to_thread(reader.join, 2)


@pytest.mark.asyncio
@pytest.mark.parametrize("retirement", ["exited", "stopped"])
@pytest.mark.parametrize("received_reply", [None, "result", "error"])
async def test_restart_detaches_old_requests_before_reusing_ids(monkeypatch, retirement, received_reply):
    transport = PersistentTransport("unused", timeout=2)
    old, old_lines, _old_writes = _callback_process()
    current, current_lines, current_writes = _callback_process()
    children = iter([old, current])
    monkeypatch.setattr("meerkat_mobkit._transport.subprocess.Popen", lambda *args, **kwargs: next(children))
    old_written, release_old, current_written = threading.Event(), threading.Event(), threading.Event()
    write = transport._write_line

    def delayed_return(obj, **kwargs):
        write(obj, **kwargs)
        if obj.get("params", {}).get("owner") == "old":
            old_written.set()
            assert release_old.wait(2)
        else:
            current_written.set()

    monkeypatch.setattr(transport, "_write_line", delayed_return)
    readers = []
    requests = []
    old_reply = None
    try:
        transport.start()
        readers.append(transport._reader_thread)
        old_request = asyncio.create_task(transport.send_async({
            "jsonrpc": "2.0", "id": "reused-id", "method": "mobkit/init", "params": {"owner": "old"},
        }))
        requests.append(old_request)
        assert await asyncio.to_thread(old_written.wait, 2)
        old_event = transport._pending["reused-id"]
        if received_reply is not None:
            old_reply = {"jsonrpc": "2.0", "id": "reused-id", received_reply: {
                "old": True, "stdio_shutdown_handshake": True, "stdio_shutdown_horizon_ms": 123_000,
            }}
            old_lines.put(json.dumps(old_reply).encode() + b"\n")
            assert await asyncio.to_thread(old_event.wait, 2)
        if retirement == "stopped":
            transport.stop()
        else:
            old.poll.return_value = 0
        transport.start()
        readers.append(transport._reader_thread)
        old_lines.put(b"")
        await asyncio.to_thread(readers[0].join, 2)
        assert not readers[0].is_alive()
        assert old_event.is_set(), "replacement left the retired request waiting for its timeout"

        current_request = asyncio.create_task(transport.send_async({
            "jsonrpc": "2.0", "id": "reused-id", "method": "mobkit/status", "params": {"owner": "current"},
        }))
        requests.append(current_request)
        assert await asyncio.to_thread(current_written.wait, 0.5), "old request still owns the replacement request id"
        current_event = transport._pending["reused-id"]
        assert current_event is not old_event

        # Complete old cleanup only after the new child owns the reused ID.
        release_old.set()
        if old_reply is None:
            with pytest.raises(TransportReaderFailedError):
                await old_request
        else:
            assert await old_request == old_reply
        assert transport._supports_shutdown_handshake is False, "old init changed the replacement's capabilities"
        assert transport._pending["reused-id"] is current_event
        assert not current_event.is_set()
        assert len(current_writes) == 1
        current_reply = {"jsonrpc": "2.0", "id": "reused-id", "result": {"current": True}}
        current_lines.put(json.dumps(current_reply).encode() + b"\n")
        assert await current_request == current_reply
        assert transport._pending == {}
        assert transport._results == {}
    finally:
        release_old.set()
        old_lines.put(b"")
        transport._supports_shutdown_handshake = False
        transport.stop()
        await asyncio.gather(*requests, return_exceptions=True)
        for reader in readers:
            await asyncio.to_thread(reader.join, 2)


@pytest.mark.asyncio
async def test_restart_fails_only_old_init_watch(monkeypatch):
    transport = PersistentTransport("unused")
    old, old_lines, _old_writes = _callback_process()
    current, current_lines, _current_writes = _callback_process()
    children = iter([old, current])
    monkeypatch.setattr("meerkat_mobkit._transport.subprocess.Popen", lambda *args, **kwargs: next(children))
    readers = []
    try:
        transport.start()
        readers.append(transport._reader_thread)
        old_watch = transport.open_init_watch("reused-init")
        old.poll.return_value = 0
        transport.start()
        readers.append(transport._reader_thread)
        current_watch = transport.open_init_watch("reused-init")
        old_lines.put(b"")
        await asyncio.to_thread(readers[0].join, 2)
        with pytest.raises(TransportReaderFailedError):
            await old_watch.settlement(deadline=0.25)
        transport.close_init_watch(old_watch)
        assert transport._init_watch is current_watch
        settled = {"init_id": "reused-init", "status": "settled"}
        current_lines.put(json.dumps({
            "jsonrpc": "2.0", "method": "mobkit/init_settled", "params": settled,
        }).encode() + b"\n")
        assert await current_watch.settlement(deadline=1) == settled
    finally:
        old_lines.put(b"")
        transport.stop()
        for reader in readers:
            await asyncio.to_thread(reader.join, 2)


@pytest.mark.asyncio
async def test_first_start_keeps_preinstalled_init_watch(monkeypatch):
    transport = PersistentTransport("unused")
    child, lines, _writes = _callback_process()
    monkeypatch.setattr("meerkat_mobkit._transport.subprocess.Popen", lambda *args, **kwargs: child)
    watch = transport.open_init_watch("first-init")
    settled = {"init_id": "first-init", "status": "settled"}

    def answer_init(data):
        request = json.loads(data)
        lines.put(json.dumps({"jsonrpc": "2.0", "id": request["id"], "result": {"accepted": True}}).encode() + b"\n")
        lines.put(json.dumps({"jsonrpc": "2.0", "method": "mobkit/init_settled", "params": settled}).encode() + b"\n")

    child.stdin.write.side_effect = answer_init
    try:
        await transport.send_async({"jsonrpc": "2.0", "id": "first-init", "method": "mobkit/init"})
        assert await watch.settlement(deadline=1) == settled
        assert transport._init_watch is watch
    finally:
        transport.stop()
        await asyncio.to_thread(transport._reader_thread.join, 2)


@pytest.mark.asyncio
async def test_start_does_not_publish_stderr_before_its_child(monkeypatch):
    transport = PersistentTransport("unused", env={"MOBKIT_GATEWAY_STDERR_FILE": "unused.stderr"})
    old, old_lines, _old_writes = _callback_process()
    current, _current_lines, _current_writes = _callback_process()
    old_stderr, current_stderr = MagicMock(), MagicMock()
    children = iter([old, current])
    stderr_files = iter([old_stderr, current_stderr])
    preparing, publish = threading.Event(), threading.Event()
    monkeypatch.setattr("builtins.open", lambda *args, **kwargs: next(stderr_files))

    def spawn(*args, **kwargs):
        child = next(children)
        if child is current:
            assert kwargs["stderr"] is current_stderr
            preparing.set()
            assert publish.wait(2)
        return child

    monkeypatch.setattr("meerkat_mobkit._transport.subprocess.Popen", spawn)
    transport.start()
    old_reader = transport._reader_thread
    old.poll.return_value = 0
    starting = asyncio.create_task(asyncio.to_thread(transport.start))
    try:
        assert await asyncio.to_thread(preparing.wait, 2)
        transport.stop()
        current_stderr.close.assert_not_called()
        old_stderr.close.assert_called_once_with()
        publish.set()
        await starting
        assert transport._process is current
        assert transport._stderr_file is current_stderr
        current_stderr.close.assert_not_called()
    finally:
        publish.set()
        await starting
        transport.stop()
        old_lines.put(b"")
        await asyncio.to_thread(old_reader.join, 2)
        await asyncio.to_thread(transport._reader_thread.join, 2)
