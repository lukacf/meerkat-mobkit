"""MobKit runtime object -- the running instance returned by the builder."""
from __future__ import annotations

from typing import Any, AsyncIterator


class MobKitRuntime:
    """Running MobKit runtime instance.

    Created by ``MobKit.builder().build()``.  Manages the persistent
    mobkit-rpc subprocess and exposes the runtime API surface.
    """

    def __init__(self, config: Any, transport: Any = None):
        self._config = config
        self._transport = transport
        self._running = False

    @classmethod
    async def _create(cls, config: Any) -> MobKitRuntime:
        """Internal factory called by the builder."""
        runtime = cls(config)
        await runtime._bootstrap()
        return runtime

    async def _bootstrap(self) -> None:
        """Start the persistent subprocess and bootstrap the runtime."""
        self._running = True
        # Future: start persistent transport, send bootstrap RPC

    def mob_handle(self) -> MobHandle:
        """Get the mob handle for direct Meerkat API access."""
        return MobHandle(self)

    def sse_bridge(self) -> SseBridge:
        """Get the SSE bridge for streaming events."""
        return SseBridge(self)

    def asgi(
        self,
        *,
        console: bool = True,
        auth: Any | None = None,
    ) -> Any:
        """Build an ASGI application (for FastAPI/Starlette).

        Returns a Starlette/FastAPI app with:
        - Health check endpoint
        - SSE streaming endpoints
        - Console UI (if *console* is ``True``)
        - Auth middleware (if *auth* is provided)
        """
        # Future: construct and return ASGI app
        return None

    async def serve(
        self,
        app: Any = None,
        *,
        host: str = "0.0.0.0",
        port: int = 8080,
    ) -> None:
        """Start serving the ASGI application.

        If *app* is ``None``, builds one with default settings.
        """
        if app is None:
            app = self.asgi()
        # Future: use uvicorn or similar to serve

    async def shutdown(self) -> None:
        """Gracefully shut down the runtime."""
        self._running = False
        if self._transport is not None:
            # Future: stop persistent transport
            pass

    @property
    def is_running(self) -> bool:
        return self._running


class MobHandle:
    """Proxy for the Meerkat MobHandle API."""

    def __init__(self, runtime: MobKitRuntime):
        self._runtime = runtime

    async def wire(self, source_id: str, target_id: str) -> None:
        """Wire two agents together."""
        # Future: send RPC

    async def inject(self, member_id: str, message: str) -> str:
        """Inject a message into a member.  Returns interaction_id."""
        # Future: send RPC
        return ""

    async def discover(self) -> list[dict[str, Any]]:
        """List all mob members."""
        # Future: send RPC
        return []


class SseBridge:
    """Bridge for streaming SSE events from the Rust runtime."""

    def __init__(self, runtime: MobKitRuntime):
        self._runtime = runtime

    async def agent_events(self, agent_id: str) -> AsyncIterator[dict[str, Any]]:
        """Stream events for a specific agent."""
        # Future: connect to per-agent SSE endpoint
        return
        yield  # Make it an async generator

    async def mob_events(self) -> AsyncIterator[dict[str, Any]]:
        """Stream all mob events (merged)."""
        # Future: connect to mob SSE endpoint
        return
        yield
