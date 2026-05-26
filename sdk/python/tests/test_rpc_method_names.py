"""Regression tests: verify Python SDK methods call the correct RPC names.

These tests mock the transport layer and verify that each SDK method sends
the expected RPC method name. This catches mismatches between the Python SDK
and the Rust RPC dispatch table.
"""

import asyncio
import json
from unittest.mock import AsyncMock, MagicMock

import pytest
from meerkat_mobkit.errors import NotConnectedError, RpcError


def make_mock_mob_handle(rpc_responses=None):
    """Create a MobHandle with a mocked RPC transport."""
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


def make_http_mob_handle():
    from meerkat_mobkit.runtime import MobHandle

    runtime = MagicMock()
    runtime.rust_http_base_url = "http://127.0.0.1:8765"
    handle = MobHandle.__new__(MobHandle)
    handle._runtime = runtime
    return handle


class FakeHttpResponse:
    def __init__(self, payload):
        self.payload = payload

    def __enter__(self):
        return self

    def __exit__(self, exc_type, exc, tb):
        return False

    def read(self):
        return json.dumps(self.payload).encode("utf-8")


@pytest.mark.asyncio
async def test_attach_session_rpc_name():
    """P1 regression: attach_session must call mobkit/attach_existing_session."""
    handle, calls = make_mock_mob_handle({
        "mobkit/attach_existing_session": {
            "status": "active",
            "tokens_used": 0,
            "is_final": False,
        }
    })
    await handle.attach_session("worker", "w1", "sid_abc123")
    assert calls[0][0] == "mobkit/attach_existing_session"


@pytest.mark.asyncio
async def test_send_with_attachments_uses_multipart(monkeypatch):
    from meerkat_mobkit import runtime as runtime_module

    captured = {}

    def fake_urlopen(req, timeout=60):
        captured["url"] = req.full_url
        body = req.data.decode("utf-8", errors="replace")
        captured["body"] = body
        assert 'name="payload"' in body
        assert 'name="file:upload-1"' in body
        assert '"method": "mobkit/send_message"' in body
        assert '"type": "image_upload"' in body
        return FakeHttpResponse({
            "jsonrpc": "2.0",
            "id": "test",
            "result": {"accepted": True, "member_id": "m-1", "session_id": "s-1"},
        })

    monkeypatch.setattr(runtime_module.urllib_request, "urlopen", fake_urlopen)
    handle = make_http_mob_handle()
    result = await handle.send("m-1", "look", attachments=[b"png"])
    assert captured["url"] == "http://127.0.0.1:8765/console/rpc/multipart"
    assert result.accepted is True
    assert result.session_id == "s-1"


@pytest.mark.asyncio
async def test_send_with_structured_content_and_attachment_forwards_mode(monkeypatch):
    from meerkat_mobkit import runtime as runtime_module

    def fake_urlopen(req, timeout=60):
        body = req.data.decode("utf-8", errors="replace")
        assert '"handling_mode": "steer"' in body
        assert '"type": "text"' in body
        assert '"type": "image_upload"' in body
        assert '"media_type": "image/jpeg"' in body
        return FakeHttpResponse({
            "jsonrpc": "2.0",
            "id": "test",
            "result": {"accepted": True, "member_id": "m-1", "session_id": "s-2"},
        })

    monkeypatch.setattr(runtime_module.urllib_request, "urlopen", fake_urlopen)
    handle = make_http_mob_handle()
    result = await handle.send(
        "m-1",
        content=[{"type": "text", "text": "hello"}],
        attachments=[(b"jpg", "image/jpeg", "photo.jpg")],
        handling_mode="steer",
    )
    assert result.session_id == "s-2"


@pytest.mark.asyncio
async def test_send_attachment_requires_http_base():
    from meerkat_mobkit.runtime import MobHandle

    runtime = MagicMock()
    runtime.rust_http_base_url = None
    handle = MobHandle.__new__(MobHandle)
    handle._runtime = runtime
    with pytest.raises(NotConnectedError):
        await handle.send("m-1", "x", attachments=[b"png"])


@pytest.mark.asyncio
async def test_upload_blob_uses_multipart(monkeypatch):
    from meerkat_mobkit import runtime as runtime_module

    def fake_urlopen(req, timeout=60):
        body = req.data.decode("utf-8", errors="replace")
        assert '"method": "mobkit/blob/upload"' in body
        assert '"media_type": "image/png"' in body
        return FakeHttpResponse({
            "jsonrpc": "2.0",
            "id": "test",
            "result": {"blob_id": "sha256:abc", "media_type": "image/png", "size": 3},
        })

    monkeypatch.setattr(runtime_module.urllib_request, "urlopen", fake_urlopen)
    handle = make_http_mob_handle()
    result = await handle.upload_blob(b"png", media_type="image/png", filename="a.png")
    assert result["blob_id"] == "sha256:abc"
    assert result["size"] == 3


@pytest.mark.asyncio
async def test_upload_blob_raises_rpc_error(monkeypatch):
    from meerkat_mobkit import runtime as runtime_module

    def fake_urlopen(req, timeout=60):
        return FakeHttpResponse({
            "jsonrpc": "2.0",
            "id": "test",
            "error": {"code": -32602, "message": "bad upload", "data": {"reason": "unit"}},
        })

    monkeypatch.setattr(runtime_module.urllib_request, "urlopen", fake_urlopen)
    handle = make_http_mob_handle()
    with pytest.raises(RpcError) as exc:
        await handle.upload_blob(b"png", media_type="image/png")
    assert exc.value.code == -32602
    assert exc.value.method == "mobkit/blob/upload"


@pytest.mark.asyncio
async def test_upload_blob_requires_http_base():
    from meerkat_mobkit.runtime import MobHandle

    runtime = MagicMock()
    runtime.rust_http_base_url = None
    handle = MobHandle.__new__(MobHandle)
    handle._runtime = runtime
    with pytest.raises(NotConnectedError):
        await handle.upload_blob(b"png", media_type="image/png")


@pytest.mark.asyncio
async def test_collect_completed_parses_wrapped_response():
    """P2 regression: collect_completed response is {"completed": [...]}, not bare list."""
    handle, calls = make_mock_mob_handle({
        "mobkit/collect_completed": {
            "completed": [
                {
                    "member_id": "w1",
                    "snapshot": {
                        "status": "completed",
                        "tokens_used": 100,
                        "is_final": True,
                    },
                }
            ]
        }
    })
    result = await handle.collect_completed()
    assert calls[0][0] == "mobkit/collect_completed"
    assert len(result) == 1
    assert result[0][0] == "w1"
    assert result[0][1].status == "completed"
    assert result[0][1].is_final is True


@pytest.mark.asyncio
async def test_collect_completed_empty():
    """collect_completed returns empty list when no members are terminal."""
    handle, calls = make_mock_mob_handle({
        "mobkit/collect_completed": {"completed": []}
    })
    result = await handle.collect_completed()
    assert result == []


@pytest.mark.asyncio
async def test_member_status_rpc_name():
    handle, calls = make_mock_mob_handle({
        "mobkit/member_status": {
            "status": "active",
            "tokens_used": 42,
            "is_final": False,
        }
    })
    result = await handle.member_status("w1")
    assert calls[0][0] == "mobkit/member_status"
    assert result.tokens_used == 42


@pytest.mark.asyncio
async def test_force_cancel_member_rpc_name():
    handle, calls = make_mock_mob_handle()
    await handle.force_cancel_member("w1")
    assert calls[0][0] == "mobkit/force_cancel_member"


@pytest.mark.asyncio
async def test_spawn_helper_rpc_name():
    handle, calls = make_mock_mob_handle({
        "mobkit/spawn_helper": {"output": "done", "tokens_used": 10}
    })
    result = await handle.spawn_helper("h1", "do stuff", role="worker")
    assert calls[0][0] == "mobkit/spawn_helper"
    assert calls[0][1]["agent_identity"] == "h1"
    assert calls[0][1]["task"] == "do stuff"
    assert calls[0][1]["options"]["role"] == "worker"
    assert result.output == "done"


@pytest.mark.asyncio
async def test_fork_helper_rpc_name_and_params():
    handle, calls = make_mock_mob_handle({
        "mobkit/fork_helper": {"output": "forked result", "tokens_used": 5}
    })
    result = await handle.fork_helper(
        "lead",
        "fork-1",
        "review this",
        fork_context={"type": "last_messages", "count": 10},
        runtime_mode="turn_driven",
    )
    assert calls[0][0] == "mobkit/fork_helper"
    assert calls[0][1]["source_member_id"] == "lead"
    assert calls[0][1]["fork_context"] == {"type": "last_messages", "count": 10}
    assert calls[0][1]["options"]["runtime_mode"] == "turn_driven"


@pytest.mark.asyncio
async def test_cancel_flow_rpc_name():
    handle, calls = make_mock_mob_handle()
    await handle.cancel_flow("run_123")
    assert calls[0][0] == "mobkit/cancel_flow"


@pytest.mark.asyncio
async def test_flow_status_rpc_name():
    handle, calls = make_mock_mob_handle({
        "mobkit/flow_status": {
            "run_id": "r1",
            "mob_id": "m1",
            "flow_id": "f1",
            "status": "running",
            "step_ledger": [],
            "failure_ledger": [],
        }
    })
    result = await handle.flow_status("r1")
    assert calls[0][0] == "mobkit/flow_status"
    assert result.status == "running"


@pytest.mark.asyncio
async def test_list_flows_rpc_name():
    """list_flows must call mobkit/list_flows and parse the {flows: [...]} envelope."""
    handle, calls = make_mock_mob_handle({
        "mobkit/list_flows": {"flows": ["demo", "pipeline"]}
    })
    result = await handle.list_flows()
    assert calls[0][0] == "mobkit/list_flows"
    assert calls[0][1] is None
    assert result == ["demo", "pipeline"]


@pytest.mark.asyncio
async def test_run_flow_rpc_name_and_params():
    """run_flow must call mobkit/run_flow with flow_id+params and return run_id."""
    handle, calls = make_mock_mob_handle({
        "mobkit/run_flow": {"run_id": "run_abc123"}
    })
    run_id = await handle.run_flow("demo", {"choice": "a"})
    assert calls[0][0] == "mobkit/run_flow"
    assert calls[0][1] == {"flow_id": "demo", "params": {"choice": "a"}}
    assert run_id == "run_abc123"


@pytest.mark.asyncio
async def test_list_runs_rpc_name_no_filter():
    """list_runs must call mobkit/list_runs with no flow_id and parse the full ledger."""
    handle, calls = make_mock_mob_handle({
        "mobkit/list_runs": {
            "runs": [
                {
                    "run_id": "r-1",
                    "mob_id": "m-1",
                    "flow_id": "alpha",
                    "status": "completed",
                    "flow_state": {"phase": "done"},
                    "activation_params": {"k": "v"},
                    "created_at": "2026-04-29T12:00:00Z",
                    "completed_at": "2026-04-29T12:00:05Z",
                    "step_ledger": [
                        {
                            "step_id": "s1",
                            "agent_identity": "lead",
                            "status": "completed",
                            "output": {"text": "ok"},
                            "timestamp": "2026-04-29T12:00:01Z",
                        }
                    ],
                    "failure_ledger": [],
                    "frames": {
                        "frame-1": {"kernel_state": {"opaque": True}},
                    },
                    "loops": {
                        "loop-1": {"kernel_state": {}},
                    },
                    "loop_iteration_ledger": [
                        {
                            "loop_instance_id": "loop-1",
                            "iteration": 0,
                            "frame_id": "frame-2",
                        }
                    ],
                    "schema_version": 4,
                    "root_step_outputs": {"s1": {"text": "ok"}},
                    "loop_iteration_outputs": {"loop-1": []},
                }
            ]
        }
    })
    runs = await handle.list_runs()
    assert calls[0][0] == "mobkit/list_runs"
    assert calls[0][1] == {}
    assert len(runs) == 1
    run = runs[0]
    assert run.run_id == "r-1"
    assert run.flow_id == "alpha"
    assert run.status.value == "completed"
    assert run.activation_params == {"k": "v"}
    assert run.completed_at == "2026-04-29T12:00:05Z"
    assert len(run.step_ledger) == 1
    assert run.step_ledger[0].step_id == "s1"
    # frames / loops are MAPS keyed by id, not arrays.
    assert "frame-1" in run.frames
    assert "loop-1" in run.loops
    assert run.loop_iteration_ledger[0].iteration == 0
    assert run.schema_version == 4


@pytest.mark.asyncio
async def test_list_runs_rpc_name_with_flow_id_filter():
    """list_runs(flow_id=...) must forward flow_id."""
    handle, calls = make_mock_mob_handle({"mobkit/list_runs": {"runs": []}})
    runs = await handle.list_runs(flow_id="alpha")
    assert calls[0][0] == "mobkit/list_runs"
    assert calls[0][1] == {"flow_id": "alpha"}
    assert runs == []


@pytest.mark.asyncio
async def test_query_mob_events_stale_raises_typed_error():
    """A -32010 RpcError must be reified into MobEventsStaleError carrying both cursors."""
    from meerkat_mobkit.errors import MobEventsStaleError, RpcError

    async def stale_rpc(method, params=None):
        raise RpcError(
            code=-32010,
            message="stale mob event cursor: requested 999, latest 42",
            request_id="rid",
            method=method,
            data={"after_cursor": 999, "latest_cursor": 42},
        )

    from meerkat_mobkit.runtime import MobHandle

    runtime = MagicMock()
    runtime._rpc = stale_rpc
    handle = MobHandle.__new__(MobHandle)
    handle._runtime = runtime

    with pytest.raises(MobEventsStaleError) as info:
        await handle.query_mob_events({"after_seq": 999})

    assert info.value.after_cursor == 999
    assert info.value.latest_cursor == 42
    assert info.value.code == -32010


def test_console_timeline_replay_unavailable_uses_distinct_typed_error():
    """Console timeline replay gaps must not be reified as MobEventsStaleError."""
    from meerkat_mobkit.errors import ConsoleTimelineReplayUnavailableError
    from meerkat_mobkit.runtime import _rpc_error_from_payload

    err = _rpc_error_from_payload(
        {
            "code": -32013,
            "message": "query_timeline failed: replay unavailable",
            "data": {"error": "replay_unavailable"},
        },
        request_id="rid",
        method="mobkit/console/query_timeline",
    )

    assert isinstance(err, ConsoleTimelineReplayUnavailableError)
    assert err.code == -32013
    assert err.method == "mobkit/console/query_timeline"


@pytest.mark.asyncio
async def test_wait_ready_rpc_name():
    """wait_ready must call mobkit/wait_ready and forward timeout in ms."""
    handle, calls = make_mock_mob_handle({
        "mobkit/wait_ready": {"ready": [], "timeout": False}
    })
    result = await handle.wait_ready(timeout=2.5)
    assert calls[0][0] == "mobkit/wait_ready"
    assert calls[0][1] == {"timeout_ms": 2500}
    assert result == {"ready": [], "timeout": False}



@pytest.mark.asyncio
async def test_peer_pubkey_rpc_name():
    """peer_pubkey must call mobkit/peer_pubkey and unwrap pubkey_b64."""
    handle, calls = make_mock_mob_handle({
        "mobkit/peer_pubkey": {"pubkey_b64": "AAAA"}
    })
    result = await handle.peer_pubkey()
    assert calls[0][0] == "mobkit/peer_pubkey"
    assert result == "AAAA"


@pytest.mark.asyncio
async def test_scheduling_evaluate_and_dispatch_rpc_names():
    handle, calls = make_mock_mob_handle({
        "mobkit/scheduling/evaluate": {"due": []},
        "mobkit/scheduling/dispatch": {"dispatched": []},
    })

    evaluate = await handle.scheduling_evaluate([{"id": "daily"}], 1234)
    dispatch = await handle.scheduling_dispatch([{"id": "daily"}], 5678)

    assert calls[0][0] == "mobkit/scheduling/evaluate"
    assert calls[0][1] == {"schedules": [{"id": "daily"}], "tick_ms": 1234}
    assert calls[1][0] == "mobkit/scheduling/dispatch"
    assert calls[1][1] == {"schedules": [{"id": "daily"}], "tick_ms": 5678}
    assert evaluate == {"due": []}
    assert dispatch == {"dispatched": []}


@pytest.mark.asyncio
async def test_session_store_bigquery_rpc_name():
    handle, calls = make_mock_mob_handle({
        "mobkit/session_store/bigquery": {"rows": 1}
    })

    result = await handle.session_store_bigquery(operation="probe")

    assert calls[0][0] == "mobkit/session_store/bigquery"
    assert calls[0][1] == {"operation": "probe"}
    assert result == {"rows": 1}


@pytest.mark.asyncio
async def test_wire_local_forwards_optional_pubkey():
    """wire_local must forward remote_pubkey_b64 when provided."""
    handle, calls = make_mock_mob_handle()
    await handle.wire_local(
        "alice",
        "remote-name",
        "00000000-0000-4000-8000-000000000001",
        "tcp://10.0.0.2:9001",
        remote_pubkey_b64="KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=",
    )
    assert calls[0][0] == "mobkit/cross_mob/wire_local"
    params = calls[0][1]
    assert params["remote_pubkey_b64"] == "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio="
    assert params["remote_address"] == "tcp://10.0.0.2:9001"


@pytest.mark.asyncio
async def test_wire_local_omits_pubkey_when_absent():
    """Backward compat: inproc-only callers must not see remote_pubkey_b64
    leak into the wire-local params dict."""
    handle, calls = make_mock_mob_handle()
    await handle.wire_local(
        "alice",
        "remote-name",
        "00000000-0000-4000-8000-000000000001",
        "inproc://remote-name",
    )
    assert calls[0][0] == "mobkit/cross_mob/wire_local"
    assert "remote_pubkey_b64" not in calls[0][1]
