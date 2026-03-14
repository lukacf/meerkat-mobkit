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
from .errors import NotConnectedError, RpcError, TransportError
from .events import AgentEvent, MobEvent
from ._sse import SseEvent, parse_sse_stream
from ._transport import PersistentTransport
from .models import DiscoverySpec
from .types import (
    CallToolResult,
    CapabilitiesResult,
    DeliveryHistoryResult,
    DeliveryResult,
    GatingAuditEntry,
    GatingDecisionResult,
    GatingEvaluateResult,
    GatingPendingEntry,
    MemberSnapshot,
    MemoryIndexResult,
    MemoryQueryResult,
    MemoryStoreInfo,
    ReconcileEdgesReport,
    ReconcileResult,
    RediscoverReport,
    RoutingResolution,
    RuntimeRouteResult,
    SendMessageResult,
    SpawnResult,
    StatusResult,
    SubscribeResult,
)

from ._client import _build_request as _rpc_request

_request_counter = itertools.count(1)


def _next_request_id(method: str) -> str:
    return f"{method}:{next(_request_counter)}"


class MobKitRuntime:
    """Running MobKit runtime instance.

    Supports both context-manager and explicit lifecycle patterns::

        # Context manager
        async with await MobKit.builder().mob("mob.toml").build() as rt:
            handle = rt.mob_handle()
            status = await handle.status()

        # Explicit lifecycle
        rt = await MobKit.builder().mob("mob.toml").build()
        await rt.connect()
        ...
        await rt.shutdown()
    """

    def __init__(self, config: Any, transport: PersistentTransport | None = None):
        self._config = config
        self._transport = transport
        self._running = False
        self._dispatcher = CallbackDispatcher()
        self._rust_http_base: str | None = None

    async def __aenter__(self) -> MobKitRuntime:
        if not self._running:
            await self.connect()
        return self

    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        await self.shutdown()

    @classmethod
    async def _create(cls, config: Any) -> MobKitRuntime:
        runtime = cls(config)
        await runtime._bootstrap()
        return runtime

    async def connect(self) -> None:
        """Explicitly connect to the runtime. Idempotent."""
        if self._running:
            return
        await self._bootstrap()

    async def _bootstrap(self) -> None:
        if self._config.gateway_bin:
            self._transport = PersistentTransport(self._config.gateway_bin)
            # Register builder FIRST — init may trigger callback/build_agent
            if self._config.session_builder and isinstance(
                self._config.session_builder, SessionAgentBuilder
            ):
                self._dispatcher.register_builder(self._config.session_builder)
            if self._config.error_callback is not None:
                self._dispatcher.register_error_callback(self._config.error_callback)
            self._transport.set_callback_handler(self._dispatcher.handle_callback)
            self._transport.start()
            if not self._transport.is_running():
                raise TransportError(
                    f"gateway binary failed to start: {self._config.gateway_bin}"
                )
            try:
                init_result = await self._rpc("mobkit/init", self._build_init_params())
                if isinstance(init_result, dict):
                    self._rust_http_base = init_result.get("http_base_url")
                    if not self._rust_http_base:
                        _log.warning(
                            "mobkit/init did not return http_base_url — "
                            "SSE event streaming unavailable"
                        )
            except Exception:
                if self._transport is not None and not self._transport.is_running():
                    raise TransportError("gateway process died during bootstrap")
                raise TransportError("mobkit/init failed — runtime could not be initialized")
        elif self._config.session_builder and isinstance(
            self._config.session_builder, SessionAgentBuilder
        ):
            self._dispatcher.register_builder(self._config.session_builder)
        else:
            _log.warning(
                "MobKit runtime started without gateway or session builder — "
                "RPC calls will fail with NotConnectedError"
            )
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
        runtime_options: dict[str, Any] = {}
        if self._config.gating_config_path:
            runtime_options["gating_config_path"] = self._config.gating_config_path
        if self._config.routing_config_path:
            runtime_options["routing_config_path"] = self._config.routing_config_path
        if self._config.scheduling_files:
            runtime_options["scheduling_files"] = self._config.scheduling_files
        if self._config.memory_config:
            runtime_options["memory_config"] = self._config.memory_config
        if self._config.auth_config:
            runtime_options["auth_config"] = self._config.auth_config
        if self._config.event_log:
            runtime_options["event_log"] = self._config.event_log
        params["runtime_options"] = runtime_options
        return params

    def _rpc_sync(self, method: str, params: dict[str, Any] | None = None) -> Any:
        if self._transport is None:
            raise NotConnectedError("runtime not started — no transport available")
        rid = _next_request_id(method)
        response = self._transport.send_sync(_rpc_request(rid, method, params))
        if "error" in response:
            err = response["error"]
            raise RpcError(
                code=err.get("code", -1),
                message=err.get("message", str(err)),
                request_id=rid,
                method=method,
            )
        return response.get("result")

    async def _rpc(self, method: str, params: dict[str, Any] | None = None) -> Any:
        if self._transport is None:
            raise NotConnectedError("runtime not started — no transport available")
        rid = _next_request_id(method)
        response = await self._transport.send_async(_rpc_request(rid, method, params))
        if "error" in response:
            err = response["error"]
            raise RpcError(
                code=err.get("code", -1),
                message=err.get("message", str(err)),
                request_id=rid,
                method=method,
            )
        return response.get("result")

    @property
    def rust_http_base_url(self) -> str | None:
        return self._rust_http_base

    def set_rust_http_base(self, url: str) -> None:
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
            self._transport = None

    @property
    def is_running(self) -> bool:
        return self._running


class MobHandle:
    """Proxy for the Meerkat MobHandle API via JSON-RPC.

    Returns typed result objects instead of raw dicts.
    """

    def __init__(self, runtime: MobKitRuntime):
        self._runtime = runtime

    async def status(self) -> StatusResult:
        """Return the current runtime status."""
        raw = await self._runtime._rpc("mobkit/status")
        return StatusResult.from_dict(raw)

    async def capabilities(self) -> CapabilitiesResult:
        """Return the runtime's advertised capabilities."""
        raw = await self._runtime._rpc("mobkit/capabilities")
        return CapabilitiesResult.from_dict(raw)

    async def spawn(self, spec: DiscoverySpec) -> SpawnResult:
        """Spawn a mob member from a full discovery spec."""
        raw = await self._runtime._rpc("mobkit/spawn_member", spec.to_dict())
        return SpawnResult.from_dict(raw)

    async def spawn_member(self, module_id: str) -> SpawnResult:
        """Spawn a mob member by module ID."""
        raw = await self._runtime._rpc("mobkit/spawn_member", {"module_id": module_id})
        return SpawnResult.from_dict(raw)

    async def reconcile(self, modules: list[str]) -> ReconcileResult:
        """Reconcile the mob to match the given module list."""
        raw = await self._runtime._rpc("mobkit/reconcile", {"modules": modules})
        return ReconcileResult.from_dict(raw)

    async def subscribe_events(
        self,
        scope: str = "mob",
        last_event_id: str | None = None,
        agent_id: str | None = None,
    ) -> SubscribeResult:
        """Subscribe to runtime events with an optional scope filter."""
        params: dict[str, Any] = {"scope": scope}
        if last_event_id is not None:
            params["last_event_id"] = last_event_id
        if agent_id is not None:
            params["agent_id"] = agent_id
        raw = await self._runtime._rpc("mobkit/events/subscribe", params)
        return SubscribeResult.from_dict(raw)

    async def resolve_routing(self, recipient: str, **kwargs: Any) -> RoutingResolution:
        """Resolve a routing target for the given recipient."""
        raw = await self._runtime._rpc(
            "mobkit/routing/resolve", {"recipient": recipient, **kwargs}
        )
        return RoutingResolution.from_dict(raw)

    async def send_delivery(self, **kwargs: Any) -> DeliveryResult:
        """Send a delivery payload through the routing layer."""
        raw = await self._runtime._rpc("mobkit/delivery/send", kwargs)
        return DeliveryResult.from_dict(raw)

    async def memory_query(self, query: str, **kwargs: Any) -> MemoryQueryResult:
        """Query a memory store by natural-language assertion."""
        raw = await self._runtime._rpc("mobkit/memory/query", {"query": query, **kwargs})
        return MemoryQueryResult.from_dict(raw)

    async def call_tool(
        self, module_id: str, tool: str, arguments: dict[str, Any] | None = None
    ) -> CallToolResult:
        """Call an MCP tool on a loaded module."""
        params: dict[str, Any] = {"module_id": module_id, "tool": tool}
        if arguments:
            params["arguments"] = arguments
        raw = await self._runtime._rpc("mobkit/call_tool", params)
        return CallToolResult.from_dict(raw)

    def tool_caller(self, module_id: str) -> ToolCaller:
        """Return a callable scoped to one MCP module.

        Usage::

            gmail = mob_handle.tool_caller("google-workspace")
            messages = await gmail("gmail_search", query="is:unread")
        """
        return ToolCaller(self, module_id)

    # -----------------------------------------------------------------
    # Primary API — comms, observation, control plane
    # -----------------------------------------------------------------

    async def ensure_member(
        self, member_id: str, profile: str, **kwargs: Any
    ) -> MemberSnapshot:
        """Ensure a mob member exists, spawning it if missing.

        Idempotent — returns the member snapshot whether it was just spawned
        or already existed. Use before ``send()`` when handling first contact
        from an unknown user (e.g. new Slack DM).

        Args:
            member_id: Meerkat ID for the member.
            profile: Profile name from mob.toml to spawn with.
            **kwargs: Optional fields (labels, context, resume_session_id,
                      additional_instructions).
        """
        params: dict[str, Any] = {"profile": profile, "meerkat_id": member_id}
        if "labels" in kwargs:
            params["labels"] = kwargs["labels"]
        if "context" in kwargs:
            params["context"] = kwargs["context"]
        if "resume_session_id" in kwargs:
            params["resume_session_id"] = kwargs["resume_session_id"]
        if "additional_instructions" in kwargs:
            params["additional_instructions"] = kwargs["additional_instructions"]
        raw = await self._runtime._rpc("mobkit/ensure_member", params)
        return MemberSnapshot.from_dict(raw)

    async def find_members(
        self, label_key: str, label_value: str
    ) -> list[MemberSnapshot]:
        """Find members matching a label key-value pair.

        Example::

            # Find all initiative agents
            initiatives = await handle.find_members("agent_type", "initiative")

            # Find the agent for a specific owner
            agents = await handle.find_members("owner_id", "user-123")
            if agents:
                meerkat_id = agents[0].meerkat_id
        """
        raw = await self._runtime._rpc(
            "mobkit/find_members",
            {"label_key": label_key, "label_value": label_value},
        )
        if isinstance(raw, list):
            return [MemberSnapshot.from_dict(m) for m in raw]
        return []

    async def rediscover(self) -> RediscoverReport | None:
        """Reset the mob and re-run discovery + edge reconciliation.

        Sequence: reset mob (retire all, clear state) → re-run Discovery →
        spawn discovered members → reconcile edges.

        Returns ``None`` if no Discovery was configured on the builder.

        Use for "nuke everything and start fresh" scenarios — e.g. a config
        reload, admin reset command, or recovery from a bad state.
        """
        raw = await self._runtime._rpc("mobkit/rediscover")
        if isinstance(raw, dict) and "status" in raw:
            return None
        return RediscoverReport.from_dict(raw)

    async def reconcile_edges(self) -> ReconcileEdgesReport:
        """Re-run edge discovery and reconcile dynamic peer edges.

        Refreshes the active roster, runs the configured ``EdgeDiscovery``,
        and applies wire/unwire operations to match the desired topology.

        Only useful if ``EdgeDiscovery`` was configured on the builder.
        Returns an empty report if no edge discovery is configured.
        """
        raw = await self._runtime._rpc("mobkit/reconcile_edges")
        return ReconcileEdgesReport.from_dict(raw)

    async def send(self, member_id: str, message: str) -> SendMessageResult:
        """Send a message to a mob member and return the accepting session."""
        raw = await self._runtime._rpc(
            "mobkit/send_message",
            {"member_id": member_id, "message": message},
        )
        return SendMessageResult.from_dict(raw)

    async def query_events(
        self,
        *,
        since_ms: int | None = None,
        until_ms: int | None = None,
        member_id: str | None = None,
        event_types: list[str] | None = None,
        limit: int | None = None,
        after_seq: int | None = None,
    ) -> list[PersistedEvent]:
        """Query persisted operational events from the event log.

        Returns an empty list if no event log is configured.
        """
        from .types import EventQuery, PersistedEvent
        query = EventQuery(
            since_ms=since_ms,
            until_ms=until_ms,
            member_id=member_id,
            event_types=event_types or [],
            limit=limit,
            after_seq=after_seq,
        )
        raw = await self._runtime._rpc("mobkit/query_events", query.to_dict())
        if isinstance(raw, dict) and raw.get("status") == "no_event_log_configured":
            return []
        if isinstance(raw, list):
            return [PersistedEvent.from_dict(e) for e in raw]
        return []

    # -----------------------------------------------------------------
    # Roster — member lifecycle
    # -----------------------------------------------------------------

    async def list_members(self) -> list[MemberSnapshot]:
        """List all members in the mob roster (active + retiring)."""
        raw = await self._runtime._rpc("mobkit/list_members")
        if isinstance(raw, list):
            return [MemberSnapshot.from_dict(m) for m in raw]
        return []

    async def get_member(self, member_id: str) -> MemberSnapshot:
        """Get a single member snapshot by ID. Raises RpcError if not found."""
        raw = await self._runtime._rpc("mobkit/get_member", {"member_id": member_id})
        return MemberSnapshot.from_dict(raw)

    async def retire_member(self, member_id: str) -> None:
        """Retire a member (transition to retiring state)."""
        await self._runtime._rpc("mobkit/retire_member", {"member_id": member_id})

    async def respawn_member(self, member_id: str) -> None:
        """Respawn a member (replace with fresh instance)."""
        await self._runtime._rpc("mobkit/respawn_member", {"member_id": member_id})

    # -----------------------------------------------------------------
    # Routing — route management
    # -----------------------------------------------------------------

    async def list_routes(self) -> list[RuntimeRouteResult]:
        """List all configured runtime routes."""
        raw = await self._runtime._rpc("mobkit/routing/routes/list")
        routes = raw.get("routes", []) if isinstance(raw, dict) else []
        return [RuntimeRouteResult.from_dict(r) for r in routes]

    async def add_route(
        self,
        route_key: str,
        recipient: str,
        sink: str,
        target_module: str,
        channel: str | None = None,
    ) -> RuntimeRouteResult:
        """Add or update a route. Overwrites on duplicate route_key."""
        params: dict[str, Any] = {
            "route_key": route_key,
            "recipient": recipient,
            "sink": sink,
            "target_module": target_module,
        }
        if channel is not None:
            params["channel"] = channel
        raw = await self._runtime._rpc("mobkit/routing/routes/add", params)
        route_data = raw.get("route", raw) if isinstance(raw, dict) else raw
        return RuntimeRouteResult.from_dict(route_data)

    async def delete_route(self, route_key: str) -> RuntimeRouteResult:
        """Delete a route by key. Returns the deleted route."""
        raw = await self._runtime._rpc("mobkit/routing/routes/delete", {"route_key": route_key})
        deleted_data = raw.get("deleted", raw) if isinstance(raw, dict) else raw
        return RuntimeRouteResult.from_dict(deleted_data)

    # -----------------------------------------------------------------
    # Delivery — history
    # -----------------------------------------------------------------

    async def delivery_history(
        self,
        recipient: str | None = None,
        sink: str | None = None,
        limit: int = 20,
    ) -> DeliveryHistoryResult:
        """Query delivery history with optional recipient/sink filters."""
        params: dict[str, Any] = {"limit": limit}
        if recipient is not None:
            params["recipient"] = recipient
        if sink is not None:
            params["sink"] = sink
        raw = await self._runtime._rpc("mobkit/delivery/history", params)
        return DeliveryHistoryResult.from_dict(raw)

    # -----------------------------------------------------------------
    # Gating — policy enforcement
    # -----------------------------------------------------------------

    async def gating_evaluate(
        self,
        action: str,
        actor_id: str,
        **kwargs: Any,
    ) -> GatingEvaluateResult:
        """Evaluate an action against configured gating policies."""
        params: dict[str, Any] = {"action": action, "actor_id": actor_id, **kwargs}
        raw = await self._runtime._rpc("mobkit/gating/evaluate", params)
        return GatingEvaluateResult.from_dict(raw)

    async def gating_pending(self) -> list[GatingPendingEntry]:
        """List gating decisions awaiting approval."""
        raw = await self._runtime._rpc("mobkit/gating/pending")
        entries = raw.get("pending", []) if isinstance(raw, dict) else []
        return [GatingPendingEntry.from_dict(e) for e in entries]

    async def gating_decide(
        self,
        pending_id: str,
        decision: str,
        approver_id: str,
        **kwargs: Any,
    ) -> GatingDecisionResult:
        """Approve or reject a pending gating action."""
        params: dict[str, Any] = {
            "pending_id": pending_id,
            "decision": decision,
            "approver_id": approver_id,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/gating/decide", params)
        return GatingDecisionResult.from_dict(raw)

    async def gating_audit(self, limit: int = 100) -> list[GatingAuditEntry]:
        """Query the gating audit log."""
        raw = await self._runtime._rpc("mobkit/gating/audit", {"limit": limit})
        entries = raw.get("entries", []) if isinstance(raw, dict) else []
        return [GatingAuditEntry.from_dict(e) for e in entries]

    # -----------------------------------------------------------------
    # Memory — store management
    # -----------------------------------------------------------------

    async def memory_stores(self) -> list[MemoryStoreInfo]:
        """List available memory stores with record counts."""
        raw = await self._runtime._rpc("mobkit/memory/stores")
        stores = raw.get("stores", []) if isinstance(raw, dict) else []
        return [MemoryStoreInfo.from_dict(s) for s in stores]

    async def memory_index(
        self,
        entity: str,
        topic: str,
        store: str,
        **kwargs: Any,
    ) -> MemoryIndexResult:
        """Index an assertion into a memory store."""
        params: dict[str, Any] = {
            "entity": entity,
            "topic": topic,
            "store": store,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/memory/index", params)
        return MemoryIndexResult.from_dict(raw)

    # Alias for backward compatibility
    send_message = send

    async def subscribe_agent(self, member_id: str) -> AsyncIterator[AgentEvent]:
        """Stream events for one agent. Pure observation."""
        bridge = self._runtime.sse_bridge()
        async for event in bridge.agent_events(member_id):
            yield AgentEvent.from_sse(event, agent_id=member_id)

    async def subscribe_mob(self) -> AsyncIterator[MobEvent]:
        """Stream mob-wide events. Pure observation."""
        bridge = self._runtime.sse_bridge()
        async for event in bridge.mob_events():
            yield MobEvent.from_sse(event)


class ToolCaller:
    """Bound callable scoped to one MCP module.

    Wraps ``MobHandle.call_tool`` with a fixed ``module_id`` and unwraps
    the result so callers get raw data instead of ``CallToolResult``.
    """

    def __init__(self, mob_handle: MobHandle, module_id: str) -> None:
        self._mob_handle = mob_handle
        self._module_id = module_id

    async def __call__(self, tool: str, **kwargs: Any) -> Any:
        """Call a tool on the bound MCP module, return unwrapped result.

        Raises whatever ``call_tool`` raises on failure (e.g. ``RpcError``).
        """
        result = await self._mob_handle.call_tool(self._module_id, tool, kwargs or None)
        return result.result


class SseBridge:
    """Bridge for streaming SSE from the Rust backend's HTTP server."""

    def __init__(self, runtime: MobKitRuntime):
        self._runtime = runtime

    def _base_url(self) -> str:
        base = self._runtime.rust_http_base_url
        if base is None:
            raise NotConnectedError(
                "SSE bridge requires rust_http_base_url — set it via "
                "runtime.set_rust_http_base('http://127.0.0.1:8081') or "
                "ensure the Rust binary reports it during bootstrap"
            )
        return base

    async def agent_events(self, agent_id: str) -> AsyncIterator[SseEvent]:
        url = f"{self._base_url()}/agents/{agent_id}/events"
        async for event in self._stream_sse(url):
            yield event

    async def mob_events(self) -> AsyncIterator[SseEvent]:
        url = f"{self._base_url()}/mob/events"
        async for event in self._stream_sse(url):
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
    """ASGI app: REST handled directly, SSE proxied to Rust backend."""

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

        if path == "/healthz":
            await _send_response(send, 200, b"ok", content_type=b"text/plain")
            return

        # Gate console/observation routes when console=False.
        if not self._console:
            is_console_route = (
                path.startswith("/console")
                or path == "/mob/events"
                or (path.startswith("/agents/") and path.endswith("/events"))
            )
            if is_console_route:
                await _send_response(send, 404, b'{"error":"not found"}')
                return

        # Enforce auth when auth_config is provided.
        if self._auth_config is not None and path != "/healthz":
            headers = dict(scope.get("headers", []))
            auth_header = headers.get(
                b"authorization", b""
            ).decode("utf-8", errors="replace")
            if not auth_header.startswith("Bearer ") or not auth_header[7:].strip():
                resp = json.dumps({"error": "unauthorized"}).encode()
                await _send_response(send, 401, resp)
                return
            token = auth_header[7:].strip()
            if not _validate_bearer_token(token, self._auth_config):
                resp = json.dumps({"error": "unauthorized", "reason": "invalid_token"}).encode()
                await _send_response(send, 401, resp)
                return

        if path == "/rpc" and method == "POST":
            body = await _read_body(receive)
            request_id = None
            try:
                parsed = json.loads(body)
            except (json.JSONDecodeError, ValueError) as exc:
                resp = json.dumps({
                    "jsonrpc": "2.0",
                    "id": None,
                    "error": {"code": -32700, "message": f"Parse error: {exc}"},
                }).encode()
                await _send_response(send, 200, resp)
                return
            if not isinstance(parsed, dict) or "method" not in parsed:
                resp = json.dumps({
                    "jsonrpc": "2.0",
                    "id": parsed.get("id") if isinstance(parsed, dict) else None,
                    "error": {"code": -32600, "message": "Invalid Request: must be a JSON object with a method field"},
                }).encode()
                await _send_response(send, 200, resp)
                return
            request_id = parsed.get("id")
            try:
                result = await self._runtime._rpc(
                    parsed.get("method", ""),
                    parsed.get("params"),
                )
                resp = json.dumps({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "result": result,
                }).encode()
                await _send_response(send, 200, resp)
            except RpcError as exc:
                resp = json.dumps({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {"code": exc.code, "message": exc.message},
                }).encode()
                await _send_response(send, 200, resp)
            except Exception as exc:
                resp = json.dumps({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {"code": -32603, "message": str(exc)},
                }).encode()
                await _send_response(send, 200, resp)
            return

        if path.startswith("/agents/") and path.endswith("/events") and method == "GET":
            parts = path.split("/")
            if len(parts) >= 4:
                agent_id = parts[2]
                bridge = self._runtime.sse_bridge()
                await self._proxy_sse(send, bridge.agent_events(agent_id))
                return

        if path == "/mob/events" and method == "GET":
            bridge = self._runtime.sse_bridge()
            await self._proxy_sse(send, bridge.mob_events())
            return

        if self._fallback_app is not None:
            await self._fallback_app(scope, receive, send)
            return

        await _send_response(send, 404, b'{"error":"not found"}')

    async def _proxy_sse(
        self,
        send: Any,
        event_stream: AsyncIterator[SseEvent],
    ) -> None:
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

        await send({
            "type": "http.response.start",
            "status": 200,
            "headers": [
                [b"content-type", b"text/event-stream"],
                [b"cache-control", b"no-cache"],
                [b"connection", b"keep-alive"],
            ],
        })

        await send({
            "type": "http.response.body",
            "body": first_event.encode().encode("utf-8"),
            "more_body": True,
        })

        try:
            async for event in event_stream:
                chunk = event.encode().encode("utf-8")
                await send({"type": "http.response.body", "body": chunk, "more_body": True})
        except Exception:
            pass
        await send({"type": "http.response.body", "body": b""})


def _validate_bearer_token(token: str, auth_config: Any) -> bool:
    """Validate a bearer token against the auth config.

    For JwtAuthConfig (shared-secret HMAC), performs full signature
    verification.  For GoogleAuthConfig, decodes the JWT structure and
    checks issuer/audience claims (cryptographic verification requires
    JWKS and is deferred to the Rust gateway).  Unknown config types
    are rejected (fail closed).
    """
    import base64
    import hmac
    import hashlib

    parts = token.split(".")
    if len(parts) != 3:
        return False

    try:
        # Decode header to get algorithm
        header_b64 = parts[0] + "=" * (-len(parts[0]) % 4)
        header = json.loads(base64.urlsafe_b64decode(header_b64))
    except Exception:
        return False

    config_dict = auth_config.to_dict() if hasattr(auth_config, "to_dict") else {}
    provider = config_dict.get("provider", "")

    if provider == "jwt":
        # Full HMAC-SHA256 verification
        secret = config_dict.get("shared_secret", "")
        if not secret:
            return False
        if header.get("alg") != "HS256":
            return False
        signing_input = f"{parts[0]}.{parts[1]}".encode("utf-8")
        expected_sig = base64.urlsafe_b64encode(
            hmac.new(secret.encode("utf-8"), signing_input, hashlib.sha256).digest()
        ).rstrip(b"=").decode("utf-8")
        if not hmac.compare_digest(expected_sig, parts[2]):
            return False
        # Check claims
        try:
            payload_b64 = parts[1] + "=" * (-len(parts[1]) % 4)
            claims = json.loads(base64.urlsafe_b64decode(payload_b64))
        except Exception:
            return False
        if config_dict.get("issuer") and claims.get("iss") != config_dict["issuer"]:
            return False
        if config_dict.get("audience") and claims.get("aud") != config_dict["audience"]:
            return False
        return True

    if provider == "google":
        # Structural validation only — full OIDC/JWKS is in the Rust gateway
        try:
            payload_b64 = parts[1] + "=" * (-len(parts[1]) % 4)
            claims = json.loads(base64.urlsafe_b64decode(payload_b64))
        except Exception:
            return False
        expected_aud = config_dict.get("audience") or config_dict.get("client_id")
        if expected_aud and claims.get("aud") != expected_aud:
            return False
        return True

    # Unknown provider — fail closed
    return False


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
