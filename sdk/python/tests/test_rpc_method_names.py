"""Regression tests: verify Python SDK methods call the correct RPC names.

These tests mock the transport layer and verify that each SDK method sends
the expected RPC method name. This catches mismatches between the Python SDK
and the Rust RPC dispatch table.
"""

import asyncio
from unittest.mock import AsyncMock, MagicMock

import pytest


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
async def test_wait_ready_rpc_name():
    """wait_ready must call mobkit/wait_ready and forward timeout in ms."""
    handle, calls = make_mock_mob_handle({
        "mobkit/wait_ready": {"ready": [], "timeout": False}
    })
    result = await handle.wait_ready(timeout=2.5)
    assert calls[0][0] == "mobkit/wait_ready"
    assert calls[0][1] == {"timeout_ms": 2500}
    assert result == {"ready": [], "timeout": False}


