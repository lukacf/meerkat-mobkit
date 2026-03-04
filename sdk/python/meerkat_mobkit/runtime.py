"""MobKit runtime object — the running instance returned by the builder."""
from __future__ import annotations

import asyncio
import itertools
import json
import logging
from typing import Any, AsyncIterator
from urllib import request as urllib_request

_log = logging.getLogger("meerkat_mobkit")

from .agent_builder import CallbackDispatcher, SessionAgentBuilder
from .sse import SseEvent, parse_sse_stream
from .transport import PersistentTransport


from .client import _build_request as _rpc_request

_request_counter = itertools.count(1)


def _next_request_id(method: str) -> str:
    return f"{method}:{next(_request_counter)}"


class MobKitRuntime:
    """Running MobKit runtime instance.

    The Rust runtime (mobkit-rpc binary) runs its own HTTP server for SSE
    endpoints. The Python ASGI app is a separate frontend that proxies
    SSE routes to the Rust backend and handles REST routes directly.

    Architecture::

        Browser → Python ASGI (uvicorn :8080)
                    ├─ /healthz, /rpc → handled directly via RPC transport
                    ├─ /agents/{id}/events → HTTP proxy to Rust :8081
                    ├─ /mob/events → HTTP proxy to Rust :8081
                    ├─ /interactions/stream → HTTP proxy to Rust :8081
                    └─ /* → fallback_app (Starlette/FastAPI)

        Python ←──RPC──→ Rust (mobkit-rpc subprocess, stdio JSON-RPC)
        Rust HTTP server (:8081) serves SSE endpoints directly
    """

    def __init__(self, config: Any, transport: PersistentTransport | None = None):
        self._config = config
        self._transport = transport
        self._running = False
        self._dispatcher = CallbackDispatcher()
        self._rust_http_base: str | None = None

    @classmethod
    async def _create(cls, config: Any) -> MobKitRuntime:
        runtime = cls(config)
        await runtime._bootstrap()
        return runtime

    async def _bootstrap(self) -> None:
        if self._config.gateway_bin:
            self._transport = PersistentTransport(self._config.gateway_bin)
            # Register builder FIRST — init may trigger callback/build_agent
            if self._config.session_builder and isinstance(
                self._config.session_builder, SessionAgentBuilder
            ):
                self._dispatcher.register_builder(self._config.session_builder)
            self._transport.set_callback_handler(self._dispatcher.handle_callback)
            self._transport.start()
            if not self._transport.is_running():
                raise RuntimeError(
                    f"gateway binary failed to start: {self._config.gateway_bin}"
                )
            # Send config as init — Rust bootstraps and returns HTTP port
            try:
                init_result = self._rpc_sync("mobkit/init", self._build_init_params())
                if isinstance(init_result, dict):
                    self._rust_http_base = init_result.get("http_base_url")
                    if not self._rust_http_base:
                        _log.warning(
                            "mobkit/init did not return http_base_url — "
                            "SSE features (inject_and_subscribe, event streaming) unavailable"
                        )
            except Exception:
                if self._transport is not None and not self._transport.is_running():
                    raise RuntimeError("gateway process died during bootstrap")
                raise RuntimeError("mobkit/init failed — runtime could not be initialized")
        elif self._config.session_builder and isinstance(
            self._config.session_builder, SessionAgentBuilder
        ):
            self._dispatcher.register_builder(self._config.session_builder)
        self._running = True

    def _build_init_params(self) -> dict[str, Any]:
        """Build init params dict from builder config for mobkit/init RPC."""
        params: dict[str, Any] = {}
        if self._config.mob_config_path:
            with open(self._config.mob_config_path) as f:
                params["mob_config"] = f.read()
        if self._config.modules:
            params["modules"] = self._config.modules
        params["has_session_builder"] = bool(self._config.session_builder)
        params["runtime_options"] = {}
        return params

    def _rpc_sync(self, method: str, params: dict[str, Any] | None = None) -> Any:
        if self._transport is None:
            raise RuntimeError("runtime not started — no transport available")
        rid = _next_request_id(method)
        response = self._transport.send_sync(_rpc_request(rid, method, params))
        if "error" in response:
            raise RuntimeError(f"RPC error: {response['error']}")
        return response.get("result")

    async def _rpc(self, method: str, params: dict[str, Any] | None = None) -> Any:
        if self._transport is None:
            raise RuntimeError("runtime not started — no transport available")
        rid = _next_request_id(method)
        response = await self._transport.send_async(_rpc_request(rid, method, params))
        if "error" in response:
            raise RuntimeError(f"RPC error: {response['error']}")
        return response.get("result")

    @property
    def rust_http_base_url(self) -> str | None:
        """Base URL of the Rust HTTP server (e.g. http://127.0.0.1:8081).

        This is the *Rust backend* port, not the Python ASGI port.
        Set automatically during bootstrap if the Rust binary reports it,
        or manually via ``set_rust_http_base(url)``.
        """
        return self._rust_http_base

    def set_rust_http_base(self, url: str) -> None:
        """Manually set the Rust backend's HTTP base URL."""
        self._rust_http_base = url

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

        REST routes are handled directly via RPC. SSE routes are proxied
        to the Rust backend's HTTP server. Unmatched paths fall through
        to extra_routes (a Starlette/FastAPI app).
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
        """Start the Python ASGI frontend."""
        if app is None:
            app = self.asgi()

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

    Method names match the Rust RPC contract exactly.
    """

    def __init__(self, runtime: MobKitRuntime):
        self._runtime = runtime

    async def status(self) -> dict[str, Any]:
        return await self._runtime._rpc("mobkit/status")

    async def capabilities(self) -> dict[str, Any]:
        return await self._runtime._rpc("mobkit/capabilities")

    async def spawn(self, member_spec: dict[str, Any]) -> dict[str, Any]:
        """Spawn a mob member from a spec dict."""
        return await self._runtime._rpc("mobkit/spawn_member", member_spec)

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

    async def inject_and_subscribe(self, target: str, message: str) -> AsyncIterator[SseEvent]:
        """Inject a message and stream SSE responses via HTTP.

        Uses the Rust HTTP SSE endpoint (not stdio RPC) because this is
        inherently streaming. ``target`` maps to ``member_id`` in the
        Rust /interactions/stream endpoint.
        Requires rust_http_base_url (guaranteed set after mobkit/init).
        """
        bridge = self._runtime.sse_bridge()
        async for event in bridge.interaction_stream(target, message):
            yield event


class SseBridge:
    """Bridge for streaming SSE from the Rust backend's HTTP server.

    Connects to the Rust binary's HTTP port (not the Python ASGI port)
    and streams events via urllib. One OS thread per open SSE connection
    via asyncio.to_thread — acceptable for moderate concurrency. For high
    concurrency, use httpx with an async HTTP client instead.
    """

    def __init__(self, runtime: MobKitRuntime):
        self._runtime = runtime

    def _base_url(self) -> str:
        base = self._runtime.rust_http_base_url
        if base is None:
            raise RuntimeError(
                "SSE bridge requires rust_http_base_url — set it via "
                "runtime.set_rust_http_base('http://127.0.0.1:8081') or "
                "ensure the Rust binary reports it during bootstrap"
            )
        return base

    async def agent_events(self, agent_id: str) -> AsyncIterator[SseEvent]:
        """Stream per-agent events. Yields SseEvent objects directly."""
        url = f"{self._base_url()}/agents/{agent_id}/events"
        async for event in self._stream_sse(url):
            yield event

    async def mob_events(self) -> AsyncIterator[SseEvent]:
        """Stream merged mob events. Yields SseEvent objects directly."""
        url = f"{self._base_url()}/mob/events"
        async for event in self._stream_sse(url):
            yield event

    async def interaction_stream(
        self, member_id: str, message: str
    ) -> AsyncIterator[SseEvent]:
        """POST /interactions/stream and yield SseEvent objects."""
        url = f"{self._base_url()}/interactions/stream"
        body = json.dumps({"member_id": member_id, "message": message}).encode()
        async for event in self._stream_sse(url, method="POST", body=body):
            yield event

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
    """ASGI app: REST handled directly, SSE proxied to Rust backend.

    The browser talks to one host (the Python ASGI port). REST routes use
    the RPC transport. SSE routes are proxied to the Rust backend's HTTP
    server via SseBridge (which connects to a *different* port).

    Unmatched paths fall through to fallback_app (Starlette/FastAPI).
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
        self._fallback_app = self._normalize_fallback(fallback_app)

    @staticmethod
    def _normalize_fallback(app: Any) -> Any:
        if app is None:
            return None
        if callable(app):
            return app
        if isinstance(app, list):
            try:
                from starlette.applications import Starlette
            except ImportError:
                raise ImportError(
                    "extra_routes is a list of Route objects but starlette is not installed. "
                    "Install starlette or pass an ASGI app directly."
                )
            return Starlette(routes=app)
        return app

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

        # --- JSON-RPC (async, not blocking) ---
        if path == "/rpc" and method == "POST":
            body = await _read_body(receive)
            try:
                parsed = json.loads(body)
                result = await self._runtime._rpc(
                    parsed.get("method", ""),
                    parsed.get("params"),
                )
                await _send_response(send, 200, json.dumps(result).encode())
            except Exception as exc:
                err = json.dumps({"error": str(exc)}).encode()
                await _send_response(send, 500, err)
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

        # --- Fallback ---
        if self._fallback_app is not None:
            await self._fallback_app(scope, receive, send)
            return

        await _send_response(send, 404, b'{"error":"not found"}')

    async def _proxy_sse(
        self,
        send: Any,
        event_stream: AsyncIterator[SseEvent],
    ) -> None:
        """Proxy SSE events from the Rust backend to the ASGI client.

        Validates the connection before sending headers by pulling the
        first event. If the bridge fails (backend unreachable), returns
        502 instead of a broken SSE stream.
        """
        try:
            first_event: SseEvent | None = None
            async for event in event_stream:
                first_event = event
                break

            if first_event is None:
                await _send_response(send, 204, b"")
                return
        except Exception as exc:
            err = json.dumps({"error": f"SSE backend unavailable: {exc}"}).encode()
            await _send_response(send, 502, err)
            return

        # Connection validated — send SSE headers and stream
        await send({
            "type": "http.response.start",
            "status": 200,
            "headers": [
                [b"content-type", b"text/event-stream"],
                [b"cache-control", b"no-cache"],
                [b"connection", b"keep-alive"],
            ],
        })

        # Send first event
        await send({
            "type": "http.response.body",
            "body": first_event.encode().encode("utf-8"),
            "more_body": True,
        })

        # Stream remaining events
        try:
            async for event in event_stream:
                chunk = event.encode().encode("utf-8")
                await send({"type": "http.response.body", "body": chunk, "more_body": True})
        except Exception:
            pass  # Client disconnect or backend failure — close cleanly
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
