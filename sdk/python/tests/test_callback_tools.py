"""Tests for callback tool dispatch (callback/call_tool)."""
from functools import partial

import pytest
from meerkat_mobkit.agent_builder import CallbackDispatcher
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
            "callback/build_agent", {"options": {"scope_id": "s1"}}
        )
        assert result["tools"] == ["sync_tool", "async_tool"]
        assert result["profile_name"] == "test"
        assert ("s1", "sync_tool") in dispatcher._tool_handlers
        assert ("s1", "async_tool") in dispatcher._tool_handlers

    @pytest.mark.asyncio
    async def test_call_sync_tool(self, dispatcher):
        await dispatcher.handle_callback(
            "callback/build_agent", {"options": {"scope_id": "s1"}}
        )
        result = await dispatcher.handle_callback(
            "callback/call_tool",
            {"scope_id": "s1", "tool": "sync_tool", "arguments": {"input": "hello"}},
        )
        assert result == {"content": {"echo": "hello"}}

    @pytest.mark.asyncio
    async def test_call_async_tool(self, dispatcher):
        await dispatcher.handle_callback(
            "callback/build_agent", {"options": {"scope_id": "s1"}}
        )
        result = await dispatcher.handle_callback(
            "callback/call_tool",
            {"scope_id": "s1", "tool": "async_tool", "arguments": {"input": "world"}},
        )
        assert result == {"content": {"async_echo": "world"}}

    @pytest.mark.asyncio
    async def test_call_unknown_tool_raises(self, dispatcher):
        await dispatcher.handle_callback(
            "callback/build_agent", {"options": {"scope_id": "s1"}}
        )
        with pytest.raises(ValueError, match="no handler registered for tool"):
            await dispatcher.handle_callback(
                "callback/call_tool",
                {"scope_id": "s1", "tool": "nonexistent", "arguments": {}},
            )

    @pytest.mark.asyncio
    async def test_call_tool_before_build_raises(self):
        d = CallbackDispatcher()
        with pytest.raises(ValueError, match="no handler registered for tool"):
            await d.handle_callback(
                "callback/call_tool",
                {"scope_id": "s1", "tool": "anything", "arguments": {}},
            )

    @pytest.mark.asyncio
    async def test_scope_isolation(self, dispatcher):
        """Tools from one scope are not visible in another scope."""
        await dispatcher.handle_callback(
            "callback/build_agent", {"options": {"scope_id": "session-A"}}
        )
        result = await dispatcher.handle_callback(
            "callback/call_tool",
            {"scope_id": "session-A", "tool": "sync_tool", "arguments": {"input": "ok"}},
        )
        assert result == {"content": {"echo": "ok"}}

        with pytest.raises(ValueError, match="no handler registered"):
            await dispatcher.handle_callback(
                "callback/call_tool",
                {"scope_id": "session-B", "tool": "sync_tool", "arguments": {}},
            )

    @pytest.mark.asyncio
    async def test_wrapped_async_handler(self):
        """Async callables that aren't detected by iscoroutinefunction still work."""
        d = CallbackDispatcher()

        async def base_handler(prefix, args):
            return f"{prefix}: {args.get('input', '')}"

        class WrappedBuilder:
            async def build_agent(self, opts: SessionBuildOptions) -> None:
                opts.register_tool("wrapped", partial(base_handler, "PREFIX"))

        d.register_builder(WrappedBuilder())
        await d.handle_callback(
            "callback/build_agent", {"options": {"scope_id": "s1"}}
        )
        result = await d.handle_callback(
            "callback/call_tool",
            {"scope_id": "s1", "tool": "wrapped", "arguments": {"input": "test"}},
        )
        assert result == {"content": "PREFIX: test"}

    @pytest.mark.asyncio
    async def test_missing_scope_id_in_build_raises(self, dispatcher):
        """build_agent without scope_id fails fast."""
        with pytest.raises(ValueError, match="requires scope_id"):
            await dispatcher.handle_callback(
                "callback/build_agent", {"options": {}}
            )

    @pytest.mark.asyncio
    async def test_missing_scope_id_in_call_raises(self, dispatcher):
        """call_tool without scope_id fails fast."""
        with pytest.raises(ValueError, match="requires scope_id"):
            await dispatcher.handle_callback(
                "callback/call_tool", {"tool": "anything", "arguments": {}}
            )

    @pytest.mark.asyncio
    async def test_release_scope_cleans_handlers(self, dispatcher):
        """release_scope removes handlers for that scope."""
        await dispatcher.handle_callback(
            "callback/build_agent", {"options": {"scope_id": "s1"}}
        )
        assert ("s1", "sync_tool") in dispatcher._tool_handlers
        dispatcher.release_scope("s1")
        assert ("s1", "sync_tool") not in dispatcher._tool_handlers
        assert "s1" not in dispatcher._scope_tools

    @pytest.mark.asyncio
    async def test_release_scope_does_not_affect_other_scopes(self, dispatcher):
        """Releasing one scope leaves other scopes intact."""
        await dispatcher.handle_callback(
            "callback/build_agent", {"options": {"scope_id": "s1"}}
        )
        await dispatcher.handle_callback(
            "callback/build_agent", {"options": {"scope_id": "s2"}}
        )
        dispatcher.release_scope("s1")
        assert ("s1", "sync_tool") not in dispatcher._tool_handlers
        assert ("s2", "sync_tool") in dispatcher._tool_handlers


class TestRegisterToolValidation:
    def test_non_callable_handler_raises(self):
        opts = SessionBuildOptions()
        with pytest.raises(TypeError, match="handler must be callable"):
            opts.register_tool("bad", "not_a_function")

    def test_none_handler_raises(self):
        opts = SessionBuildOptions()
        with pytest.raises(TypeError, match="handler must be callable"):
            opts.register_tool("bad", None)
