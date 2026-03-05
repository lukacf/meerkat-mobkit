"""Tests for callback tool dispatch (callback/call_tool)."""
import pytest
from meerkat_mobkit.agent_builder import CallbackDispatcher, SessionAgentBuilder
from meerkat_mobkit.models import SessionBuildOptions


class _TestBuilder:
    """Test builder that registers a sync and async tool."""

    async def build_agent(self, opts: SessionBuildOptions) -> None:
        opts.profile_name = "test"
        opts.register_tool("sync_tool", lambda args: {"echo": args.get("input", "")})

        async def async_handler(args):
            return {"async_echo": args.get("input", "")}

        opts.register_tool("async_tool", async_handler)


class TestCallbackToolDispatch:
    @pytest.fixture
    def dispatcher(self):
        d = CallbackDispatcher()
        d.register_builder(_TestBuilder())
        return d

    @pytest.mark.asyncio
    async def test_build_agent_captures_handlers(self, dispatcher):
        result = await dispatcher.handle_callback(
            "callback/build_agent", {"options": {}}
        )
        assert result["tools"] == ["sync_tool", "async_tool"]
        assert result["profile_name"] == "test"
        assert "sync_tool" in dispatcher._tool_handlers
        assert "async_tool" in dispatcher._tool_handlers

    @pytest.mark.asyncio
    async def test_call_sync_tool(self, dispatcher):
        # First build to register tools
        await dispatcher.handle_callback("callback/build_agent", {"options": {}})
        # Then call the tool
        result = await dispatcher.handle_callback(
            "callback/call_tool", {"tool": "sync_tool", "arguments": {"input": "hello"}}
        )
        assert result == {"content": {"echo": "hello"}}

    @pytest.mark.asyncio
    async def test_call_async_tool(self, dispatcher):
        await dispatcher.handle_callback("callback/build_agent", {"options": {}})
        result = await dispatcher.handle_callback(
            "callback/call_tool",
            {"tool": "async_tool", "arguments": {"input": "world"}},
        )
        assert result == {"content": {"async_echo": "world"}}

    @pytest.mark.asyncio
    async def test_call_unknown_tool_raises(self, dispatcher):
        await dispatcher.handle_callback("callback/build_agent", {"options": {}})
        with pytest.raises(ValueError, match="no handler registered for tool"):
            await dispatcher.handle_callback(
                "callback/call_tool", {"tool": "nonexistent", "arguments": {}}
            )

    @pytest.mark.asyncio
    async def test_call_tool_before_build_raises(self):
        d = CallbackDispatcher()
        with pytest.raises(ValueError, match="no handler registered for tool"):
            await d.handle_callback(
                "callback/call_tool", {"tool": "anything", "arguments": {}}
            )
