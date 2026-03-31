"""TDD tests for the new builder API: persistent_state, after_create, SessionCreatedContext."""
import pytest
from meerkat_mobkit.builder import MobKit, MobKitBuilder
from meerkat_mobkit.agent_builder import CallbackDispatcher, SessionAgentBuilder
from meerkat_mobkit.models import SessionBuildOptions, SessionCreatedContext


class TestPersistentState:
    """Tests for .persistent_state() on MobKitBuilder."""

    def test_persistent_state_returns_builder(self):
        b = MobKit.builder().persistent_state("/tmp/test-state")
        assert isinstance(b, MobKitBuilder)

    def test_persistent_state_sets_config(self):
        b = MobKit.builder().persistent_state("/tmp/test-state")
        assert b._config.persistent_state == "/tmp/test-state"

    def test_persistent_state_default_is_none(self):
        b = MobKit.builder()
        assert b._config.persistent_state is None


class TestAfterCreate:
    """Tests for after_create callback on SessionAgentBuilder and CallbackDispatcher."""

    @pytest.mark.asyncio
    async def test_after_create_dispatched(self):
        """callback/after_create routes to builder.after_create()."""
        received = {}

        class Builder:
            async def build_agent(self, opts: SessionBuildOptions) -> None:
                pass

            async def after_create(self, session_id: str, context: dict) -> None:
                received["session_id"] = session_id
                received["context"] = context

        d = CallbackDispatcher()
        d.register_builder(Builder())

        await d.handle_callback("callback/after_create", {
            "session_id": "sid-123",
            "model": "claude-sonnet-4-5",
            "labels": {"agent_type": "lead"},
            "system_prompt": "You are a lead.",
        })

        assert received["session_id"] == "sid-123"
        ctx = received["context"]
        assert isinstance(ctx, SessionCreatedContext)
        assert ctx.model == "claude-sonnet-4-5"
        assert ctx.labels == {"agent_type": "lead"}
        assert ctx.system_prompt == "You are a lead."

    @pytest.mark.asyncio
    async def test_after_create_noop_when_not_defined(self):
        """Builders without after_create don't fail on callback/after_create."""

        class MinimalBuilder:
            async def build_agent(self, opts: SessionBuildOptions) -> None:
                pass

        d = CallbackDispatcher()
        d.register_builder(MinimalBuilder())

        # Should not raise — gracefully ignored.
        await d.handle_callback("callback/after_create", {
            "session_id": "sid-456",
            "model": "test-model",
            "labels": {},
            "system_prompt": None,
        })

    @pytest.mark.asyncio
    async def test_after_create_error_is_logged_not_raised(self):
        """after_create failures are best-effort — logged, not propagated."""

        class FailingBuilder:
            async def build_agent(self, opts: SessionBuildOptions) -> None:
                pass

            async def after_create(self, session_id: str, context: dict) -> None:
                raise RuntimeError("db unavailable")

        d = CallbackDispatcher()
        d.register_builder(FailingBuilder())

        # Should not raise — error is logged.
        await d.handle_callback("callback/after_create", {
            "session_id": "sid-789",
            "model": "test-model",
            "labels": {},
            "system_prompt": None,
        })


class TestSessionCreatedContext:
    """Tests for SessionCreatedContext dataclass."""

    def test_import(self):
        from meerkat_mobkit import SessionCreatedContext
        ctx = SessionCreatedContext(
            model="claude-sonnet-4-5",
            labels={"agent_type": "lead"},
            system_prompt="You are a lead agent.",
        )
        assert ctx.model == "claude-sonnet-4-5"
        assert ctx.labels == {"agent_type": "lead"}
        assert ctx.system_prompt == "You are a lead agent."

    def test_from_dict(self):
        from meerkat_mobkit import SessionCreatedContext
        ctx = SessionCreatedContext.from_dict({
            "model": "test-model",
            "labels": {"k": "v"},
            "system_prompt": None,
        })
        assert ctx.model == "test-model"
        assert ctx.labels == {"k": "v"}
        assert ctx.system_prompt is None


class TestBuildAgentErrorPropagation:
    """Tests that build_agent errors propagate (not fallback)."""

    @pytest.mark.asyncio
    async def test_build_agent_error_raises(self):
        """When build_agent throws, the error should propagate up."""

        class FailingBuilder:
            async def build_agent(self, opts: SessionBuildOptions) -> None:
                raise ValueError("hook abort: invalid config")

        d = CallbackDispatcher()
        d.register_builder(FailingBuilder())

        with pytest.raises(ValueError, match="hook abort"):
            await d.handle_callback(
                "callback/build_agent",
                {"options": {"scope_id": "s1"}},
            )
