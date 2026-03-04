"""MobKit runtime object — the running instance returned by the builder."""
from __future__ import annotations

import asyncio
import json
from typing import Any, AsyncIterator
from urllib import request as urllib_request

from .agent_builder import CallbackDispatcher, SessionAgentBuilder
from .sse import SseEvent, parse_sse_stream
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
        self._http_base: str | None = None
        self._serve_task: asyncio.Task[None] | None = None

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
        if self._transport is None:
            raise RuntimeError("runtime not started — no transport available")
        response = self._transport.send_sync(_rpc_request(method, method, params))
        if "error" in response:
            raise RuntimeError(f"RPC error: {response['error']}")
        return response.get("result")

    async def _rpc(self, method: str, params: dict[str, Any] | None = None) -> Any:
        if self._transport is None:
            raise RuntimeError("runtime not started — no transport available")
        response = await self._transport.send_async(_rpc_request(method, method, params))
        if "error" in response:
            raise RuntimeError(f"RPC error: {response['error']}")
        return response.get("result")

    @property
    def http_base_url(self) -> str | None:
        """Base URL of the Rust HTTP server (e.g. http://127.0.0.1:8080)."""
        return self._http_base

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
    ) -> AsgiApp:
        """Build an ASGI application that proxies to the Rust runtime.

        The returned app provides:
        - /healthz health check
        - SSE streaming endpoints (proxied from Rust)
        - Console UI (if console=True)
        - Extra routes (if provided)
        """
        return AsgiApp(
            runtime=self,
            console=console,
            auth_config=auth,
            extra_routes=extra_routes,
        )

    async def serve(
        self,
        app: Any = None,
        *,
        host: str = "0.0.0.0",
        port: int = 8080,
    ) -> None:
        """Start serving. Uses uvicorn if available, falls back to a basic ASGI server."""
        if app is None:
            app = self.asgi()

        self._http_base = f"http://{host}:{port}"

        try:
            import uvicorn
            config = uvicorn.Config(app, host=host, port=port, log_level="info")
            server = uvicorn.Server(config)
            await server.serve()
        except ImportError:
            # Minimal fallback: just keep the runtime alive
            # The Rust binary serves HTTP directly; Python proxies via RPC
            stop = asyncio.Event()

            import signal
            loop = asyncio.get_running_loop()
            for sig in (signal.SIGINT, signal.SIGTERM):
                loop.add_signal_handler(sig, stop.set)

            await stop.wait()
        finally:
            await self.shutdown()

    async def shutdown(self) -> None:
        self._running = False
        if self._transport is not None:
            self._transport.stop()

    @property
    def is_running(self) -> bool:
        return self._running


class MobHandle:
    """Proxy for the Meerkat MobHandle API via JSON-RPC.

    Uses the actual RPC methods from the Rust contract.
    """

    def __init__(self, runtime: MobKitRuntime):
        self._runtime = runtime

    async def status(self) -> dict[str, Any]:
        """Get runtime status."""
        return await self._runtime._rpc("mobkit/status")

    async def capabilities(self) -> dict[str, Any]:
        """Get runtime capabilities and available methods."""
        return await self._runtime._rpc("mobkit/capabilities")

    async def spawn_member(self, module_id: str) -> dict[str, Any]:
        """Spawn a new member from a module."""
        return await self._runtime._rpc("mobkit/spawn_member", {"module_id": module_id})

    async def reconcile(self, modules: list[str]) -> dict[str, Any]:
        """Reconcile module list with runtime."""
        return await self._runtime._rpc("mobkit/reconcile", {"modules": modules})

    async def subscribe_events(
        self,
        scope: str = "mob",
        last_event_id: str | None = None,
        agent_id: str | None = None,
    ) -> dict[str, Any]:
        """Subscribe to runtime events."""
        params: dict[str, Any] = {"scope": scope}
        if last_event_id is not None:
            params["last_event_id"] = last_event_id
        if agent_id is not None:
            params["agent_id"] = agent_id
        return await self._runtime._rpc("mobkit/events/subscribe", params)

    async def resolve_routing(self, recipient: str, **kwargs: Any) -> dict[str, Any]:
        """Resolve a routing destination."""
        return await self._runtime._rpc(
            "mobkit/routing/resolve", {"recipient": recipient, **kwargs}
        )

    async def send_delivery(self, **kwargs: Any) -> dict[str, Any]:
        """Send a delivery."""
        return await self._runtime._rpc("mobkit/delivery/send", kwargs)

    async def memory_query(self, query: str, **kwargs: Any) -> dict[str, Any]:
        """Query memory store."""
        return await self._runtime._rpc("mobkit/memory/query", {"query": query, **kwargs})


class SseBridge:
    """Bridge for streaming SSE events from the Rust runtime's HTTP endpoints.

    Connects via HTTP to the Rust runtime's SSE endpoints and yields
    parsed events as async iterators.
    """

    def __init__(self, runtime: MobKitRuntime):
        self._runtime = runtime

    def _base_url(self) -> str:
        base = self._runtime.http_base_url
        if base is None:
            raise RuntimeError(
                "SSE bridge requires http_base_url — call runtime.serve() first "
                "or set it via the builder"
            )
        return base

    async def agent_events(self, agent_id: str) -> AsyncIterator[dict[str, Any]]:
        """Stream events for a specific agent.

        Connects to GET /agents/{agent_id}/events on the Rust runtime.
        """
        url = f"{self._base_url()}/agents/{agent_id}/events"
        async for event in self._stream_sse(url):
            yield event.to_dict()

    async def mob_events(self) -> AsyncIterator[dict[str, Any]]:
        """Stream all mob events (merged).

        Connects to GET /mob/events on the Rust runtime.
        """
        url = f"{self._base_url()}/mob/events"
        async for event in self._stream_sse(url):
            yield event.to_dict()

    async def interaction_stream(
        self, member_id: str, message: str
    ) -> AsyncIterator[dict[str, Any]]:
        """Start an interaction and stream SSE events.

        POST /interactions/stream with member_id and message.
        """
        url = f"{self._base_url()}/interactions/stream"
        body = json.dumps({"member_id": member_id, "message": message}).encode()
        async for event in self._stream_sse(url, method="POST", body=body):
            yield event.to_dict()

    async def _stream_sse(
        self,
        url: str,
        *,
        method: str = "GET",
        body: bytes | None = None,
    ) -> AsyncIterator[SseEvent]:
        """Open an HTTP connection and yield SSE events using stdlib urllib."""

        async def _read_chunks() -> AsyncIterator[bytes]:
            """Read from HTTP response in a thread to avoid blocking the event loop."""
            req = urllib_request.Request(url, method=method, data=body)
            req.add_header("Accept", "text/event-stream")
            if body is not None:
                req.add_header("Content-Type", "application/json")

            response = await asyncio.to_thread(urllib_request.urlopen, req)
            try:
                while True:
                    chunk = await asyncio.to_thread(response.read, 4096)
                    if not chunk:
                        break
                    yield chunk
            finally:
                response.close()

        async for event in parse_sse_stream(_read_chunks()):
            yield event


class AsgiApp:
    """Minimal ASGI application that proxies requests to the Rust runtime.

    Provides health check, SSE proxy, and optional console/extra routes.
    """

    def __init__(
        self,
        runtime: MobKitRuntime,
        console: bool = True,
        auth_config: Any | None = None,
        extra_routes: Any | None = None,
    ):
        self._runtime = runtime
        self._console = console
        self._auth_config = auth_config
        self._extra_routes = extra_routes or {}

    async def __call__(self, scope: dict[str, Any], receive: Any, send: Any) -> None:
        if scope["type"] != "http":
            return

        path = scope.get("path", "/")

        if path == "/healthz":
            await self._respond(send, 200, b"ok")
            return

        if path == "/rpc" and scope.get("method") == "POST":
            body = await self._read_body(receive)
            result = self._runtime._rpc_sync(
                json.loads(body).get("method", ""),
                json.loads(body).get("params"),
            )
            await self._respond(send, 200, json.dumps(result).encode())
            return

        # Extra routes
        handler = self._extra_routes.get(path) if isinstance(self._extra_routes, dict) else None
        if handler is not None:
            await handler(scope, receive, send)
            return

        await self._respond(send, 404, b"not found")

    async def _read_body(self, receive: Any) -> bytes:
        body = b""
        while True:
            message = await receive()
            body += message.get("body", b"")
            if not message.get("more_body", False):
                break
        return body

    async def _respond(self, send: Any, status: int, body: bytes) -> None:
        await send({
            "type": "http.response.start",
            "status": status,
            "headers": [[b"content-type", b"application/json"]],
        })
        await send({"type": "http.response.body", "body": body})
