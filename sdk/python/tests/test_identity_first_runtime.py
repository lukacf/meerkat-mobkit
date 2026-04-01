"""TDD tests for identity-first runtime APIs (REQ-41).

Tests that all 9 identity-first methods exist on MobKitRuntime and
delegate to the correct RPC methods with correct argument shapes.
"""
import pytest

from meerkat_mobkit.runtime import MobKitRuntime
from meerkat_mobkit.identity_first_models import DispatchInput, TextBlock


class FakeTransport:
    """Records RPC calls for assertion."""

    def __init__(self, result=None):
        self.calls: list[dict] = []
        self._result = result or {}

    def send_sync(self, request):
        self.calls.append(request)
        return {"jsonrpc": "2.0", "id": request.get("id"), "result": self._result}

    async def send_async(self, request):
        return self.send_sync(request)

    def is_running(self):
        return True

    def start(self):
        pass

    def stop(self):
        pass

    def set_callback_handler(self, handler):
        pass


def _make_runtime(result=None) -> tuple[MobKitRuntime, FakeTransport]:
    transport = FakeTransport(result=result)
    rt = MobKitRuntime.__new__(MobKitRuntime)
    rt._config = None
    rt._transport = transport
    rt._running = True
    rt._rust_http_base = None
    # Import here to avoid circular issues
    from meerkat_mobkit.agent_builder import CallbackDispatcher
    rt._dispatcher = CallbackDispatcher()
    return rt, transport


class TestIdentityFirstRuntimeAPIs:
    """REQ-41: All 9 identity-first methods exist on MobKitRuntime."""

    def test_agent_method_exists(self):
        assert hasattr(MobKitRuntime, "agent")
        assert callable(getattr(MobKitRuntime, "agent"))

    def test_send_method_exists(self):
        assert hasattr(MobKitRuntime, "send")
        assert callable(getattr(MobKitRuntime, "send"))

    def test_dispatch_method_exists(self):
        assert hasattr(MobKitRuntime, "dispatch")
        assert callable(getattr(MobKitRuntime, "dispatch"))

    def test_subscribe_method_exists(self):
        assert hasattr(MobKitRuntime, "subscribe")
        assert callable(getattr(MobKitRuntime, "subscribe"))

    def test_status_method_exists(self):
        assert hasattr(MobKitRuntime, "status")
        assert callable(getattr(MobKitRuntime, "status"))

    def test_respawn_method_exists(self):
        assert hasattr(MobKitRuntime, "respawn")
        assert callable(getattr(MobKitRuntime, "respawn"))

    def test_retire_method_exists(self):
        assert hasattr(MobKitRuntime, "retire")
        assert callable(getattr(MobKitRuntime, "retire"))

    def test_reset_method_exists(self):
        assert hasattr(MobKitRuntime, "reset")
        assert callable(getattr(MobKitRuntime, "reset"))

    def test_delete_identity_method_exists(self):
        assert hasattr(MobKitRuntime, "delete_identity")
        assert callable(getattr(MobKitRuntime, "delete_identity"))


class TestSendMethod:
    """send() accepts str or list[ContentBlock]."""

    @pytest.mark.asyncio
    async def test_send_string_content(self):
        rt, transport = _make_runtime(result={"accepted": True})
        await rt.send("triage:main", "Hello")
        assert len(transport.calls) == 1
        params = transport.calls[0]["params"]
        assert params["identity"] == "triage:main"
        assert params["content"] == "Hello"

    @pytest.mark.asyncio
    async def test_send_block_content(self):
        rt, transport = _make_runtime(result={"accepted": True})
        blocks = [TextBlock(text="Hi")]
        await rt.send("agent:alpha", blocks)
        params = transport.calls[0]["params"]
        assert params["identity"] == "agent:alpha"
        assert params["content"] == [{"type": "text", "text": "Hi"}]


class TestDispatchMethod:
    """dispatch() accepts DispatchInput."""

    @pytest.mark.asyncio
    async def test_dispatch_sends_correct_params(self):
        rt, transport = _make_runtime(result={"accepted": True})
        di = DispatchInput(content="test", origin="scheduler", correlation_id="c1")
        await rt.dispatch("gate:main", di)
        params = transport.calls[0]["params"]
        assert params["identity"] == "gate:main"
        assert params["dispatch_input"]["content"] == "test"
        assert params["dispatch_input"]["origin"] == "scheduler"
        assert params["dispatch_input"]["correlation_id"] == "c1"


class TestLifecycleMethods:
    """respawn, retire, reset, delete_identity delegate to RPC."""

    @pytest.mark.asyncio
    async def test_respawn_rpc(self):
        rt, transport = _make_runtime(result={"accepted": True})
        await rt.respawn("agent:1")
        assert transport.calls[0]["method"] == "mobkit/respawn"
        assert transport.calls[0]["params"]["identity"] == "agent:1"

    @pytest.mark.asyncio
    async def test_retire_rpc(self):
        rt, transport = _make_runtime(result={"accepted": True})
        await rt.retire("agent:1")
        assert transport.calls[0]["method"] == "mobkit/retire"
        assert transport.calls[0]["params"]["identity"] == "agent:1"

    @pytest.mark.asyncio
    async def test_reset_rpc(self):
        rt, transport = _make_runtime(result={"accepted": True})
        await rt.reset("agent:1")
        assert transport.calls[0]["method"] == "mobkit/reset"
        assert transport.calls[0]["params"]["identity"] == "agent:1"

    @pytest.mark.asyncio
    async def test_delete_identity_rpc(self):
        rt, transport = _make_runtime(result={"accepted": True})
        await rt.delete_identity("agent:1")
        assert transport.calls[0]["method"] == "mobkit/delete_identity"
        assert transport.calls[0]["params"]["identity"] == "agent:1"


class TestStatusMethod:
    """status() returns IdentityStatus."""

    @pytest.mark.asyncio
    async def test_status_rpc_and_result(self):
        raw_status = {
            "identity": "triage:main",
            "state": "active",
            "agent_runtime_id": "rt-1",
            "session_id": "s-1",
            "profile": "assistant",
            "addressability": "addressable",
            "display_name": "Triage",
            "labels": {"env": "prod"},
            "generation": 0,
            "checkpoint_version": 3,
            "lease": {
                "fencing_token": 42,
                "ttl_remaining_ms": 30000,
                "healthy": True,
            },
            "continuity_health": {
                "store_reachable": True,
                "durability_policy": {"kind": "sync_write_through"},
                "last_checkpoint_version": 3,
            },
        }
        rt, transport = _make_runtime(result=raw_status)
        from meerkat_mobkit.identity_first_models import IdentityStatus
        result = await rt.status("triage:main")
        assert isinstance(result, IdentityStatus)
        assert result.identity == "triage:main"
        assert result.state == "active"
        assert result.labels == {"env": "prod"}
        assert result.lease is not None
        assert result.lease.fencing_token == 42
        assert result.continuity_health is not None
        assert result.continuity_health.store_reachable is True


class TestAgentMethod:
    """agent() returns an AgentHandle."""

    @pytest.mark.asyncio
    async def test_agent_returns_handle(self):
        rt, transport = _make_runtime()
        from meerkat_mobkit.runtime import IdentityAgentHandle
        handle = rt.agent("triage:main")
        assert isinstance(handle, IdentityAgentHandle)
        assert handle._identity == "triage:main"


class TestSubscribeMethod:
    """subscribe() delegates to RPC."""

    @pytest.mark.asyncio
    async def test_subscribe_rpc(self):
        rt, transport = _make_runtime(result={"stream_id": "stream-1"})
        result = await rt.subscribe("agent:1")
        assert transport.calls[0]["method"] == "mobkit/subscribe"
        assert transport.calls[0]["params"]["identity"] == "agent:1"
