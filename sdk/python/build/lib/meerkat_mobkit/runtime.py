"""MobKit runtime object — the running instance returned by the builder."""
from __future__ import annotations

from typing import Any, AsyncIterator

from .agent_builder import CallbackDispatcher, SessionAgentBuilder
from .transport import PersistentTransport


class MobKitRuntime:
    """Running MobKit runtime instance.

    Created by MobKit.builder().build(). Manages the persistent
    mobkit-rpc subprocess and exposes the runtime API surface.
    """

    def __init__(self, config: Any, transport: PersistentTransport | None = None):
        self._config = config
        self._transport = transport
        self._running = False
        self._dispatcher = CallbackDispatcher()

    @classmethod
    async def _create(cls, config: Any) -> MobKitRuntime:
        runtime = cls(config)
        await runtime._bootstrap()
        return runtime

    async def _bootstrap(self) -> None:
        if self._config.gateway_bin:
            self._transport = PersistentTransport(self._config.gateway_bin)
            self._transport.start()
        if self._config.session_builder and isinstance(
            self._config.session_builder, SessionAgentBuilder
        ):
            self._dispatcher.register_builder(self._config.session_builder)
        self._running = True

    def mob_handle(self) -> MobHandle:
        return MobHandle(self)

    def sse_bridge(self) -> SseBridge:
        return SseBridge(self)

    def asgi(
        self,
        *,
        console: bool = True,
        auth: Any | None = None,
        extra_routes: Any | None = None,
    ) -> Any:
        """Build an ASGI app. Accepts extra_routes for app-defined endpoints."""
        # Future: construct Starlette/FastAPI app
        return None

    async def serve(
        self,
        app: Any = None,
        *,
        host: str = "0.0.0.0",
        port: int = 8080,
    ) -> None:
        if app is None:
            app = self.asgi()
        # Future: uvicorn.run(app, host=host, port=port)

    async def shutdown(self) -> None:
        self._running = False
        if self._transport is not None:
            self._transport.stop()

    @property
    def is_running(self) -> bool:
        return self._running


class MobHandle:
    """Proxy for the Meerkat MobHandle API."""

    def __init__(self, runtime: MobKitRuntime):
        self._runtime = runtime

    async def wire(self, source_id: str, target_id: str) -> None:
        # Future: send RPC
        pass

    async def inject(self, member_id: str, message: str) -> str:
        # Future: send RPC
        return ""

    async def discover(self) -> list[dict[str, Any]]:
        # Future: send RPC
        return []


class SseBridge:
    """Bridge for streaming SSE events from the Rust runtime."""

    def __init__(self, runtime: MobKitRuntime):
        self._runtime = runtime

    async def agent_events(self, agent_id: str) -> AsyncIterator[dict[str, Any]]:
        # Future: connect to per-agent SSE endpoint
        return
        yield  # type: ignore[misc]

    async def mob_events(self) -> AsyncIterator[dict[str, Any]]:
        # Future: connect to mob SSE endpoint
        return
        yield  # type: ignore[misc]
