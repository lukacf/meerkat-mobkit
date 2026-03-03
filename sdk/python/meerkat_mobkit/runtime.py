"""MobKit runtime object — the running instance returned by the builder."""
from __future__ import annotations

import json
from typing import Any, AsyncIterator

from .agent_builder import CallbackDispatcher, SessionAgentBuilder
from .transport import PersistentTransport


def _rpc_request(request_id: str, method: str, params: dict[str, Any] | None = None) -> dict[str, Any]:
    return {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params or {}}


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

    def _rpc_sync(self, method: str, params: dict[str, Any] | None = None) -> Any:
        """Send a JSON-RPC request and return the result. Raises on error."""
        if self._transport is None:
            raise RuntimeError("runtime not started — no transport available")
        response = self._transport.send_sync(_rpc_request(method, method, params))
        if "error" in response:
            raise RuntimeError(f"RPC error: {response['error']}")
        return response.get("result")

    async def _rpc(self, method: str, params: dict[str, Any] | None = None) -> Any:
        """Async version of _rpc_sync."""
        if self._transport is None:
            raise RuntimeError("runtime not started — no transport available")
        response = await self._transport.send_async(_rpc_request(method, method, params))
        if "error" in response:
            raise RuntimeError(f"RPC error: {response['error']}")
        return response.get("result")

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
    """Proxy for the Meerkat MobHandle API via JSON-RPC."""

    def __init__(self, runtime: MobKitRuntime):
        self._runtime = runtime

    async def wire(self, source_id: str, target_id: str) -> None:
        await self._runtime._rpc("mobkit/wire", {"source_id": source_id, "target_id": target_id})

    async def inject(self, member_id: str, message: str) -> str:
        result = await self._runtime._rpc("mobkit/inject", {"member_id": member_id, "message": message})
        return result.get("interaction_id", "") if isinstance(result, dict) else ""

    async def discover(self) -> list[dict[str, Any]]:
        result = await self._runtime._rpc("mobkit/discover")
        return result if isinstance(result, list) else []

    async def spawn(self, spec: dict[str, Any]) -> dict[str, Any]:
        return await self._runtime._rpc("mobkit/spawn_member", spec)

    async def reconcile(self, specs: list[dict[str, Any]]) -> dict[str, Any]:
        return await self._runtime._rpc("mobkit/reconcile", {"specs": specs})


class SseBridge:
    """Bridge for streaming SSE events from the Rust runtime via HTTP."""

    def __init__(self, runtime: MobKitRuntime):
        self._runtime = runtime

    async def agent_events(self, agent_id: str) -> AsyncIterator[dict[str, Any]]:
        """Stream events for a specific agent.

        Connects to GET /agents/{agent_id}/events on the Rust runtime.
        """
        # Requires HTTP SSE client — implementation depends on aiohttp/httpx
        raise NotImplementedError(
            "SSE streaming requires an HTTP client library (aiohttp or httpx). "
            "Install one and use SseEventStream from meerkat_mobkit.sse to parse the stream."
        )

    async def mob_events(self) -> AsyncIterator[dict[str, Any]]:
        """Stream all mob events (merged).

        Connects to GET /mob/events on the Rust runtime.
        """
        raise NotImplementedError(
            "SSE streaming requires an HTTP client library (aiohttp or httpx). "
            "Install one and use SseEventStream from meerkat_mobkit.sse to parse the stream."
        )
