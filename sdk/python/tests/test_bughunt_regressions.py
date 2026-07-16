"""Regression tests for the Python SDK bug-hunt fixes.

Each test pins one specific behavior identified by the bug-hunt sweep:
- AsgiApp /rpc must surface the real RPC code/message on RpcError.
- _transport.send_sync must reject empty/duplicate ids.
- EventEnvelope / KeepAliveConfig must coerce numeric fields.
- MobEvent.from_sse / AgentEvent.from_sse must preserve non-dict
  payload values instead of silently dropping them.
- subscribe_mob_events must accept both dict-envelope and bare-list
  shapes.
"""

from __future__ import annotations

import asyncio
import json
import subprocess
import threading
from typing import Any
from unittest.mock import AsyncMock, MagicMock, call, patch

import pytest

from meerkat_mobkit._sse import SseEvent
from meerkat_mobkit._transport import (
    _GATEWAY_SHUTDOWN_GRACE_SECONDS,
    _PROCESS_KILL_GRACE_SECONDS,
    _PROCESS_TERMINATE_GRACE_SECONDS,
    PersistentTransport,
)
from meerkat_mobkit.errors import RpcError
from meerkat_mobkit.events import AgentEvent, MobEvent
from meerkat_mobkit.types import EventEnvelope, KeepAliveConfig


# -- Bug #4: AsgiApp /rpc no longer raises AttributeError on RpcError --

@pytest.mark.asyncio
async def test_asgi_rpc_handler_returns_real_rpc_error_code_and_message():
    """Pre-fix: AsgiApp's /rpc handler read `exc.message` (which doesn't
    exist on RpcError), fell through to `except Exception`, and returned
    -32603 / `'RpcError' object has no attribute 'message'`.
    """
    from meerkat_mobkit.runtime import AsgiApp

    runtime = MagicMock()
    runtime._rpc = AsyncMock(
        side_effect=RpcError(
            code=-32001,
            message="boom",
            request_id="rid",
            method="mobkit/status",
            data={"detail": "x"},
        )
    )

    app = AsgiApp.__new__(AsgiApp)
    app._runtime = runtime
    app._console = True
    app._auth_config = None
    app._fallback_app = None

    request_body = json.dumps(
        {"jsonrpc": "2.0", "id": 1, "method": "mobkit/status", "params": {}}
    ).encode()

    received: list[dict] = []
    sent: list[dict] = []

    async def receive():
        return {"type": "http.request", "body": request_body, "more_body": False}

    async def send(message):
        sent.append(message)

    scope = {"type": "http", "method": "POST", "path": "/rpc", "headers": []}
    await app(scope, receive, send)

    body_bytes = b""
    for msg in sent:
        if msg.get("type") == "http.response.body":
            body_bytes += msg.get("body", b"")
    body = json.loads(body_bytes.decode())

    assert body["error"]["code"] == -32001, (
        f"AsgiApp must surface the original RPC code; got {body['error']}"
    )
    assert body["error"]["message"] == "boom"
    assert body["error"].get("data") == {"detail": "x"}


# -- Bug #15: _transport.send_sync rejects empty/duplicate ids --

def test_send_sync_rejects_request_with_no_id():
    """Pre-fix, requests without `id` collided on `_pending[""]`,
    deadlocking concurrent callers for the full timeout.
    """
    transport = PersistentTransport.__new__(PersistentTransport)
    transport._pending = {}
    transport._results = {}
    transport._pending_lock = threading.Lock()
    transport._write_lock = threading.Lock()
    transport._timeout = 1.0
    transport._process = MagicMock()  # passes _ensure_running gate
    transport._ensure_running = lambda: None

    with pytest.raises(ValueError, match="non-empty `id`"):
        transport.send_sync({"jsonrpc": "2.0", "method": "x", "params": {}})


def test_send_sync_rejects_duplicate_id_already_in_flight():
    transport = PersistentTransport.__new__(PersistentTransport)
    transport._pending = {"abc": threading.Event()}  # someone already waiting
    transport._results = {}
    transport._pending_lock = threading.Lock()
    transport._write_lock = threading.Lock()
    transport._timeout = 1.0
    transport._process = MagicMock()
    transport._ensure_running = lambda: None

    with pytest.raises(ValueError, match="already"):
        transport.send_sync({"jsonrpc": "2.0", "id": "abc", "method": "x"})


@pytest.mark.asyncio
async def test_send_async_forwards_per_request_timeout():
    transport = PersistentTransport("unused")
    transport.send_sync = MagicMock(return_value={"result": {}})
    request = {"jsonrpc": "2.0", "id": "wait-1", "method": "wait"}

    response = await transport.send_async(request, timeout=125.0)

    assert response == {"result": {}}
    transport.send_sync.assert_called_once_with(request, timeout=125.0)


def test_reader_drops_response_after_request_is_no_longer_pending():
    transport = PersistentTransport("unused")
    process = MagicMock()
    process.stdout.readline.side_effect = [
        b'{"jsonrpc":"2.0","id":"late","result":{}}\n',
        b"",
    ]
    transport._process = process
    transport._reader_loop()

    assert transport._results == {}


def test_stop_gives_gateway_full_cleanup_budget_before_termination():
    transport = PersistentTransport("unused")
    process = MagicMock()
    process.wait.side_effect = [
        subprocess.TimeoutExpired("rpc_gateway", _GATEWAY_SHUTDOWN_GRACE_SECONDS),
        subprocess.TimeoutExpired("rpc_gateway", _PROCESS_TERMINATE_GRACE_SECONDS),
        0,
    ]
    transport._process = process
    transport._request_gateway_shutdown = MagicMock(
        side_effect=RuntimeError("old gateway")
    )

    with patch(
        "meerkat_mobkit._transport.time.monotonic",
        side_effect=[100.0, 100.0],
    ):
        transport.stop()

    process.stdin.close.assert_called_once_with()
    assert process.wait.call_args_list == [
        call(timeout=_GATEWAY_SHUTDOWN_GRACE_SECONDS),
        call(timeout=_PROCESS_TERMINATE_GRACE_SECONDS),
        call(timeout=_PROCESS_KILL_GRACE_SECONDS),
    ]
    process.terminate.assert_called_once_with()
    process.kill.assert_called_once_with()
    transport._request_gateway_shutdown.assert_not_called()
    assert transport._process is None


def test_stop_keeps_stdin_open_until_gateway_shutdown_handshake_answers():
    transport = PersistentTransport("unused")
    process = MagicMock()
    process.poll.return_value = None
    process.wait.return_value = 0
    transport._process = process
    transport._supports_shutdown_handshake = True

    def answer_shutdown(request, *, timeout):
        process.stdin.close.assert_not_called()
        assert request["method"] == "mobkit/shutdown"
        assert request["id"].startswith("mobkit-shutdown-")
        assert timeout == _GATEWAY_SHUTDOWN_GRACE_SECONDS
        return {"jsonrpc": "2.0", "id": request["id"], "result": {"shutdown": True}}

    transport._send_sync_running = MagicMock(side_effect=answer_shutdown)
    with patch(
        "meerkat_mobkit._transport.time.monotonic",
        side_effect=[100.0, 100.0],
    ):
        transport.stop()

    process.stdin.close.assert_called_once_with()
    process.wait.assert_called_once_with(timeout=_GATEWAY_SHUTDOWN_GRACE_SECONDS)
    assert transport._process is None


# -- Bug #16: numeric field coercion --

def test_event_envelope_coerces_none_timestamp_to_zero():
    env = EventEnvelope.from_dict(
        {"event_id": "x", "source": "mob", "timestamp_ms": None, "event": {}}
    )
    assert env.timestamp_ms == 0
    assert isinstance(env.timestamp_ms, int)


def test_event_envelope_coerces_string_timestamp_to_int():
    env = EventEnvelope.from_dict(
        {"event_id": "x", "source": "mob", "timestamp_ms": "1700", "event": {}}
    )
    assert env.timestamp_ms == 1700
    assert isinstance(env.timestamp_ms, int)


def test_keep_alive_config_coerces_string_interval():
    cfg = KeepAliveConfig.from_dict({"interval_ms": "15", "event": "ka"})
    assert cfg.interval_ms == 15
    assert isinstance(cfg.interval_ms, int)


# -- Bug #17: from_sse non-dict payload --

def test_mob_event_from_sse_with_non_dict_payload_preserves_raw():
    sse = SseEvent(id=None, event="message", data='{"member_id":"a","payload":"hello"}')
    ev = MobEvent.from_sse(sse)
    # The agent-side parse failed (payload was a string); we should
    # carry the raw value instead of dropping it.
    assert ev.event.type == "non_dict_payload", (
        f"Expected non_dict_payload UnknownEvent, got {ev.event!r}"
    )
    assert ev.event.data == {"raw": "hello"}
    assert ev.member_id == "a"  # member_id survives


def test_agent_event_from_sse_with_string_top_level_preserves_raw():
    sse = SseEvent(id=None, event="message", data='"hi"')
    ev = AgentEvent.from_sse(sse)
    assert ev.event.type == "non_dict_payload"
    assert ev.event.data == {"raw": "hi"}


# -- Bug #24: subscribe_mob_events accepts bare list --

@pytest.mark.asyncio
async def test_subscribe_mob_events_handles_bare_list_response():
    """Pre-fix, only dict envelopes were honored; a bare list yielded
    nothing. query_mob_events handled both shapes — divergent behavior
    silently dropped events.
    """
    from meerkat_mobkit.runtime import MobHandle

    runtime = MagicMock()
    runtime._rpc = AsyncMock(
        return_value=[
            {
                "event_id": "e1",
                "cursor": 1,
                "mob_id": "m",
                "timestamp_ms": 0,
                "kind": "k",
                "data": {},
            }
        ]
    )
    handle = MobHandle.__new__(MobHandle)
    handle._runtime = runtime

    received = []
    async for ev in handle.subscribe_mob_events():
        received.append(ev)

    assert len(received) == 1, "must yield events from bare-list response"
    assert received[0].event_id == "e1"
