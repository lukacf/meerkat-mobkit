"""MobKit runtime object — the running instance returned by the builder."""
from __future__ import annotations

import asyncio
import json
from typing import Any, AsyncIterator, Callable
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
        """Base URL of the Rust HTTP server (set during serve or via builder)."""
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
        """Build an ASGI application.

        The returned app handles REST routes directly and proxies SSE routes
        to the Rust backend. One origin, one port, one deployment surface.

        Routes:
        - GET /healthz — health check
        - POST /rpc — JSON-RPC proxy to Rust
        - GET /agents/{id}/events — SSE proxy (per-agent)
        - GET /mob/events — SSE proxy (merged)
        - POST /interactions/stream — SSE proxy (interaction)

        Args:
            console: Mount console UI routes (future).
            auth: Auth config for middleware (future).
            extra_routes: A raw ASGI app to fall through to for app-defined
                routes (e.g. a Starlette/FastAPI app). If a path isn't
                handled by the built-in routes, it's forwarded here.
        """
        return AsgiApp(
            runtime=self,
            console=console,
            auth_config=auth,
            fallback_app=extra_routes,
        )

    async def serve(
        self,
        app: Any = None,
        *,
        host: str = "0.0.0.0",
        port: int = 8080,
    ) -> None:
        """Start serving. Uses uvicorn if available, falls back to signal wait."""
        if app is None:
            app = self.asgi()

        self._http_base = f"http://{host}:{port}"

        try:
            import uvicorn
            config = uvicorn.Config(app, host=host, port=port, log_level="info")
            server = uvicorn.Server(config)
            await server.serve()
        except ImportError:
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
        return await self._runtime._rpc("mobkit/status")

    async def capabilities(self) -> dict[str, Any]:
        return await self._runtime._rpc("mobkit/capabilities")

    async def spawn_member(self, module_id: str) -> dict[str, Any]:
        return await self._runtime._rpc("mobkit/spawn_member", {"module_id": module_id})

    async def reconcile(self, modules: list[str]) -> dict[str, Any]:
        return await self._runtime._rpc("mobkit/reconcile", {"modules": modules})

    async def subscribe_events(
        self,
        scope: str = "mob",
        last_event_id: str | None = None,
        agent_id: str | None = None,
    ) -> dict[str, Any]:
        params: dict[str, Any] = {"scope": scope}
        if last_event_id is not None:
            params["last_event_id"] = last_event_id
        if agent_id is not None:
            params["agent_id"] = agent_id
        return await self._runtime._rpc("mobkit/events/subscribe", params)

    async def resolve_routing(self, recipient: str, **kwargs: Any) -> dict[str, Any]:
        return await self._runtime._rpc(
            "mobkit/routing/resolve", {"recipient": recipient, **kwargs}
        )

    async def send_delivery(self, **kwargs: Any) -> dict[str, Any]:
        return await self._runtime._rpc("mobkit/delivery/send", kwargs)

    async def memory_query(self, query: str, **kwargs: Any) -> dict[str, Any]:
        return await self._runtime._rpc("mobkit/memory/query", {"query": query, **kwargs})


class SseBridge:
    """Bridge for streaming SSE events from the Rust runtime's HTTP endpoints.

    Uses stdlib urllib for zero external dependencies. Each open SSE connection
    uses one OS thread via asyncio.to_thread. For high-concurrency deployments,
    wrap the runtime with an httpx-based bridge instead.
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
        """Stream events for a specific agent from GET /agents/{id}/events."""
        url = f"{self._base_url()}/agents/{agent_id}/events"
        async for event in self._stream_sse(url):
            yield event.to_dict()

    async def mob_events(self) -> AsyncIterator[dict[str, Any]]:
        """Stream all mob events from GET /mob/events."""
        url = f"{self._base_url()}/mob/events"
        async for event in self._stream_sse(url):
            yield event.to_dict()

    async def interaction_stream(
        self, member_id: str, message: str
    ) -> AsyncIterator[dict[str, Any]]:
        """POST /interactions/stream and stream SSE events."""
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
        async def _read_chunks() -> AsyncIterator[bytes]:
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
    """ASGI application that handles REST and proxies SSE to the Rust runtime.

    The browser talks to one host. REST routes (/healthz, /rpc) are handled
    directly. SSE routes (/agents/{id}/events, /mob/events, /interactions/stream)
    are proxied to the Rust backend via SseBridge. Unmatched paths fall through
    to the fallback_app (a Starlette/FastAPI app, or any ASGI callable).

    Usage with Starlette::

        from starlette.applications import Starlette
        from starlette.routing import Route

        starlette_app = Starlette(routes=[
            Route("/my-page", my_handler),
        ])

        app = runtime.asgi(extra_routes=starlette_app)
    """

    def __init__(
        self,
        runtime: MobKitRuntime,
        console: bool = True,
        auth_config: Any | None = None,
        fallback_app: Any | None = None,
    ):
        self._runtime = runtime
        self._console = console
        self._auth_config = auth_config
        self._fallback_app = fallback_app

    async def __call__(self, scope: dict[str, Any], receive: Any, send: Any) -> None:
        if scope["type"] != "http":
            if self._fallback_app is not None:
                await self._fallback_app(scope, receive, send)
            return

        path: str = scope.get("path", "/")
        method: str = scope.get("method", "GET")

        # --- Health check ---
        if path == "/healthz":
            await _send_response(send, 200, b"ok", content_type=b"text/plain")
            return

        # --- JSON-RPC proxy ---
        if path == "/rpc" and method == "POST":
            body = await _read_body(receive)
            parsed = json.loads(body)
            result = self._runtime._rpc_sync(
                parsed.get("method", ""),
                parsed.get("params"),
            )
            await _send_response(send, 200, json.dumps(result).encode())
            return

        # --- SSE proxy: per-agent events ---
        if path.startswith("/agents/") and path.endswith("/events") and method == "GET":
            parts = path.split("/")
            if len(parts) >= 4:
                agent_id = parts[2]
                bridge = self._runtime.sse_bridge()
                await self._proxy_sse(send, bridge.agent_events(agent_id))
                return

        # --- SSE proxy: mob events ---
        if path == "/mob/events" and method == "GET":
            bridge = self._runtime.sse_bridge()
            await self._proxy_sse(send, bridge.mob_events())
            return

        # --- SSE proxy: interaction stream ---
        if path == "/interactions/stream" and method == "POST":
            body = await _read_body(receive)
            parsed = json.loads(body)
            bridge = self._runtime.sse_bridge()
            await self._proxy_sse(
                send,
                bridge.interaction_stream(
                    parsed.get("member_id", ""),
                    parsed.get("message", ""),
                ),
            )
            return

        # --- Fallback to app-defined routes (Starlette, FastAPI, etc.) ---
        if self._fallback_app is not None:
            await self._fallback_app(scope, receive, send)
            return

        await _send_response(send, 404, b'{"error":"not found"}')

    async def _proxy_sse(
        self,
        send: Any,
        event_stream: AsyncIterator[dict[str, Any]],
    ) -> None:
        """Stream SSE events to the ASGI client."""
        await send({
            "type": "http.response.start",
            "status": 200,
            "headers": [
                [b"content-type", b"text/event-stream"],
                [b"cache-control", b"no-cache"],
                [b"connection", b"keep-alive"],
            ],
        })
        async for event_dict in event_stream:
            event = SseEvent(
                id=event_dict.get("id"),
                event=event_dict.get("event", "message"),
                data=event_dict.get("data", ""),
            )
            chunk = event.encode().encode("utf-8")
            await send({"type": "http.response.body", "body": chunk, "more_body": True})
        await send({"type": "http.response.body", "body": b""})


async def _read_body(receive: Any) -> bytes:
    body = b""
    while True:
        message = await receive()
        body += message.get("body", b"")
        if not message.get("more_body", False):
            break
    return body


async def _send_response(
    send: Any,
    status: int,
    body: bytes,
    content_type: bytes = b"application/json",
) -> None:
    await send({
        "type": "http.response.start",
        "status": status,
        "headers": [[b"content-type", content_type]],
    })
    await send({"type": "http.response.body", "body": body})
