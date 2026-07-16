"""MobKit runtime object — the running instance returned by the builder."""
from __future__ import annotations

import asyncio
import base64
import hashlib
import hmac
import itertools
import json
import logging
import math
import mimetypes
import uuid
from pathlib import Path
import time
from typing import Any, AsyncIterator
from urllib import request as urllib_request
from urllib.error import HTTPError, URLError

_log = logging.getLogger("meerkat_mobkit")

from .agent_builder import CallbackDispatcher, SessionAgentBuilder
from .errors import (
    CAPABILITY_UNAVAILABLE_CODE,
    LEASE_LOST_CODE,
    MEMORY_BACKEND_UNAVAILABLE_CODE,
    MOB_EVENTS_STALE_CURSOR_CODE,
    WORKGRAPH_CONFLICT_CODE,
    WORKGRAPH_UNAVAILABLE_CODE,
    CapabilityUnavailableError,
    CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE,
    ConsoleTimelineReplayUnavailableError,
    LeaseLostError,
    MemoryBackendUnavailableError,
    MobEventsStaleError,
    NotConnectedError,
    RpcError,
    TransportError,
    WorkGraphConflictError,
    WorkGraphUnavailableError,
)
from .events import AgentEvent, MobEvent
from .identity_first_models import IdentityBootstrapStatus
from ._sse import SseEvent, parse_sse_stream
from ._transport import PersistentTransport
from .models import DiscoverySpec
from .types import (
    AgentMemoryForgetResult,
    AgentMemoryManifestResult,
    AgentMemoryRecallResult,
    AgentMemoryRecord,
    AgentMemoryRecordMeta,
    AgentMemoryUpdateResult,
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
    MobpackAgentDefinitionsResult,
    MobpackApplyOperationResult,
    MobpackCatalogsResult,
    MobpackDeployCommandResult,
    MobpackDeployResult,
    MobpackDraftDeleteResult,
    MobpackDraftGetResult,
    MobpackDraftHistoryResult,
    MobpackDraftListResult,
    MobpackDraftSaveResult,
    MobpackExportResult,
    MobpackImportResult,
    MobpackSkillsCatalogResult,
    MobpackSourceResult,
    MobpackTemplatesResult,
    MobpackToolsCatalogResult,
    MobpackValidationResult,
    MobStructuralEvent,
    ModelsCatalogResult,
    ReconcileEdgesReport,
    ReconcileResult,
    RediscoverReport,
    RoutingResolution,
    RuntimeRouteResult,
    SendMessageResult,
    SpawnResult,
    StatusResult,
    SubscribeResult,
    WorkGraphAttentionBinding,
    WorkGraphAttentionReassignResult,
    WorkGraphEdge,
    WorkGraphEventEntry,
    WorkGraphGoalResult,
    WorkGraphItem,
    WorkGraphItemsResult,
    WorkGraphSnapshotResult,
)

from ._client import _build_request as _rpc_request

_request_counter = itertools.count(1)
_IDENTITY_BOOTSTRAP_WAIT_TRANSPORT_HEADROOM_SECONDS = 5.0


def _next_request_id(method: str) -> str:
    return f"{method}:{next(_request_counter)}"


def _rpc_error_from_payload(
    err: dict[str, Any],
    *,
    request_id: str,
    method: str,
) -> RpcError:
    code = int(err.get("code", -1))
    message = str(err.get("message", err))
    data = err.get("data")
    if code == CAPABILITY_UNAVAILABLE_CODE:
        return CapabilityUnavailableError(
            message,
            request_id=request_id,
            method=method,
            data=data,
        )
    if code == LEASE_LOST_CODE:
        return LeaseLostError(
            message,
            request_id=request_id,
            method=method,
            data=data,
        )
    if code == MEMORY_BACKEND_UNAVAILABLE_CODE:
        return MemoryBackendUnavailableError(
            message,
            request_id=request_id,
            method=method,
            data=data,
        )
    if code == CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE:
        return ConsoleTimelineReplayUnavailableError(
            message,
            request_id=request_id,
            method=method,
            data=data,
        )
    if code == WORKGRAPH_UNAVAILABLE_CODE:
        return WorkGraphUnavailableError(
            message,
            request_id=request_id,
            method=method,
            data=data,
        )
    if code == WORKGRAPH_CONFLICT_CODE:
        return WorkGraphConflictError(
            message,
            detail=(data.get("detail") if isinstance(data, dict) else None),
            request_id=request_id,
            method=method,
            data=data,
        )
    base = RpcError(
        code=code,
        message=message,
        request_id=request_id,
        method=method,
        data=data,
    )
    if code == MOB_EVENTS_STALE_CURSOR_CODE:
        return MobEventsStaleError.from_rpc_error(base)
    return base


def _read_upload_source(
    source: bytes | bytearray | memoryview | str | Path,
    *,
    media_type: str | None = None,
    filename: str | None = None,
) -> tuple[bytes, str, str]:
    if isinstance(source, (str, Path)):
        path = Path(source)
        data = path.read_bytes()
        resolved_filename = filename or path.name or "attachment"
        resolved_media_type = media_type or mimetypes.guess_type(resolved_filename)[0]
    else:
        data = bytes(source)
        resolved_filename = filename or "attachment.bin"
        resolved_media_type = media_type or mimetypes.guess_type(resolved_filename)[0]
    return data, resolved_media_type or "application/octet-stream", resolved_filename


def _normalize_upload_item(
    item: Any,
    index: int,
) -> tuple[bytes, str, str]:
    if isinstance(item, dict):
        source = item.get("data", item.get("bytes", item.get("path", item.get("file"))))
        if source is None:
            raise ValueError("attachment dict requires data, bytes, path, or file")
        return _read_upload_source(
            source,
            media_type=item.get("media_type"),
            filename=item.get("filename"),
        )
    if isinstance(item, tuple) and len(item) in (2, 3):
        source = item[0]
        media_type = str(item[1]) if item[1] is not None else None
        filename = str(item[2]) if len(item) == 3 and item[2] is not None else None
        return _read_upload_source(source, media_type=media_type, filename=filename)
    if isinstance(item, (str, Path)):
        return _read_upload_source(item)
    filename = f"attachment-{index + 1}.png"
    return _read_upload_source(item, media_type="image/png", filename=filename)


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
        self._lifecycle_lock = asyncio.Lock()
        self._shutdown_task: asyncio.Task[None] | None = None

    async def __aenter__(self) -> MobKitRuntime:
        if not self._running:
            await self.connect()
        return self

    async def __aexit__(self, exc_type: Any, exc_val: Any, exc_tb: Any) -> None:
        await self.shutdown()

    @classmethod
    async def _create(cls, config: Any) -> MobKitRuntime:
        runtime = cls(config)
        await runtime.connect()
        return runtime

    async def connect(self) -> None:
        """Explicitly connect to the runtime. Idempotent."""
        async with self._lifecycle_lock:
            shutdown_task = self._shutdown_task
            if shutdown_task is not None:
                try:
                    await asyncio.shield(shutdown_task)
                finally:
                    if shutdown_task.done() and self._shutdown_task is shutdown_task:
                        self._shutdown_task = None
            if self._running:
                return
            await self._bootstrap()

    def _schedule_transport_stop(
        self, transport: PersistentTransport
    ) -> asyncio.Task[None]:
        stop_task = asyncio.create_task(asyncio.to_thread(transport.stop))
        self._shutdown_task = stop_task

        def clear_shutdown_task(done: asyncio.Task[None]) -> None:
            if self._shutdown_task is done:
                self._shutdown_task = None

        stop_task.add_done_callback(clear_shutdown_task)
        return stop_task

    async def _cleanup_failed_bootstrap(
        self, transport: PersistentTransport
    ) -> None:
        """Detach and reap a transport that failed before becoming usable.

        The stop task remains runtime-owned if the caller is cancelled. A
        later ``connect()`` therefore waits for the failed child to be fully
        reaped before it starts a replacement gateway.
        """
        self._running = False
        self._rust_http_base = None
        if self._transport is transport:
            self._transport = None

        prior_stop = self._shutdown_task
        if prior_stop is not None:
            try:
                await asyncio.shield(prior_stop)
            except Exception:
                _log.exception("prior gateway cleanup failed during bootstrap")
            finally:
                if prior_stop.done() and self._shutdown_task is prior_stop:
                    self._shutdown_task = None

        stop_task = self._schedule_transport_stop(transport)
        try:
            await asyncio.shield(stop_task)
        except Exception:
            # Preserve the bootstrap exception as the public failure. The
            # transport has still been detached, and stop() is best-effort
            # bounded cleanup even when an OS-level reap operation errors.
            _log.exception("failed to clean up gateway after bootstrap failure")
        finally:
            if stop_task.done() and self._shutdown_task is stop_task:
                self._shutdown_task = None

    def on_schedule_fire(self, name: str, handler: Any) -> None:
        """Register the handler for a named host runnable.

        Pairs with ``MobKitBuilder.host_runnables([...])``: when a durable
        schedule with a ``host_runnable`` target of that name fires, the
        gateway sends ``callback/schedule_fire`` and this handler runs with
        the occurrence dict (``schedule_id``, ``occurrence_id``, ``due_at``,
        optional ``payload``). Sync or async; raising fails the occurrence
        attempt in the durable schedule store. Register before or after
        :meth:`connect` — fires only start once a schedule targets the name.
        """
        self._dispatcher.register_schedule_fire_handler(name, handler)

    async def _bootstrap(self) -> None:
        if self._config.gateway_bin:
            transport = PersistentTransport(self._config.gateway_bin)
            self._transport = transport
            self._rust_http_base = None
            try:
                # Register builder FIRST — init may trigger callback/build_agent
                if self._config.session_builder and isinstance(
                    self._config.session_builder, SessionAgentBuilder
                ):
                    self._dispatcher.register_builder(self._config.session_builder)
                if self._config.error_callback is not None:
                    self._dispatcher.register_error_callback(self._config.error_callback)
                # Register identity-first providers before transport start —
                # restore_flow during init may trigger provider callbacks.
                if self._config.continuity_store is not None:
                    self._dispatcher.register_continuity_store(
                        self._config.continuity_store
                    )
                if self._config.lease_provider is not None:
                    self._dispatcher.register_lease_provider(self._config.lease_provider)
                if self._config.roster_provider is not None:
                    self._dispatcher.register_roster_provider(self._config.roster_provider)
                if self._config.topology_provider is not None:
                    self._dispatcher.register_topology_provider(
                        self._config.topology_provider
                    )
                if self._config.agent_customizer is not None:
                    self._dispatcher.register_agent_customizer(
                        self._config.agent_customizer
                    )
                transport.set_callback_handler(self._dispatcher.handle_callback)
                transport.start()
                if not transport.is_running():
                    raise TransportError(
                        f"gateway binary failed to start: {self._config.gateway_bin}"
                    )
                try:
                    init_result = await self._rpc(
                        "mobkit/init", self._build_init_params()
                    )
                    if isinstance(init_result, dict):
                        self._rust_http_base = init_result.get("http_base_url")
                        if not self._rust_http_base:
                            _log.warning(
                                "mobkit/init did not return http_base_url — "
                                "SSE event streaming unavailable"
                            )
                except Exception as init_err:
                    if not transport.is_running():
                        raise TransportError(
                            f"gateway process died during bootstrap: {init_err}"
                        ) from init_err
                    if isinstance(init_err, RpcError):
                        raise
                    raise TransportError(
                        f"mobkit/init failed: {init_err}"
                    ) from init_err
            except BaseException:
                await self._cleanup_failed_bootstrap(transport)
                raise
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
        if self._config.mob_config_inline:
            params["mob_config"] = self._config.mob_config_inline
        elif self._config.mob_config_path:
            with open(self._config.mob_config_path) as f:
                params["mob_config"] = f.read()
        if self._config.modules:
            params["modules"] = self._config.modules
        params["has_session_builder"] = bool(self._config.session_builder)
        runtime_options: dict[str, Any] = {}
        if self._config.gating_config_path:
            runtime_options["gating_config_path"] = self._config.gating_config_path
        if self._config.access_config_path:
            runtime_options["access_config_path"] = self._config.access_config_path
        if self._config.workgraph_enabled is not None:
            runtime_options["workgraph"] = self._config.workgraph_enabled
        if self._config.routing_config_path:
            runtime_options["routing_config_path"] = self._config.routing_config_path
        if self._config.scheduling_files:
            runtime_options["scheduling_files"] = self._config.scheduling_files
        if self._config.host_runnables:
            runtime_options["host_runnables"] = list(self._config.host_runnables)
        if self._config.memory_config:
            runtime_options["memory_config"] = _serialize_config(self._config.memory_config)
        if self._config.agent_memory_config is not None:
            runtime_options["agent_memory"] = _serialize_config(
                self._config.agent_memory_config
            )
        if self._config.auth_config:
            runtime_options["auth_config"] = _serialize_config(self._config.auth_config)
        if self._config.event_log:
            runtime_options["event_log"] = _serialize_config(self._config.event_log)
        if self._config.console_read_only is not None:
            runtime_options["console_read_only"] = self._config.console_read_only
        if self._config.console_fetch_timeout_ms is not None:
            runtime_options["console_fetch_timeout_ms"] = (
                self._config.console_fetch_timeout_ms
            )
        if self._config.implicit_delegate_idle_retire_configured:
            runtime_options["implicit_delegate_idle_retire_secs"] = (
                self._config.implicit_delegate_idle_retire_secs
            )
        if self._config.identity_bootstrap_mode is not None:
            runtime_options["identity_bootstrap_mode"] = (
                self._config.identity_bootstrap_mode.to_dict()
            )
        params["runtime_options"] = runtime_options
        if self._config.persistent_state:
            params["persistent_state"] = self._config.persistent_state
        # Identity-first provider flags
        if self._config.roster_provider is not None:
            params["has_roster_provider"] = True
        if self._config.continuity_store is not None:
            params["has_continuity_store"] = True
        if self._config.lease_provider is not None:
            params["has_lease_provider"] = True
        if self._config.scratch_dir is not None:
            params["scratch_dir"] = self._config.scratch_dir
        if self._config.topology_provider is not None:
            params["has_topology_provider"] = True
        if self._config.agent_customizer is not None:
            params["has_agent_customizer"] = True
        return params

    def _rpc_sync(self, method: str, params: dict[str, Any] | None = None) -> Any:
        if self._transport is None:
            raise NotConnectedError("runtime not started — no transport available")
        rid = _next_request_id(method)
        response = self._transport.send_sync(_rpc_request(rid, method, params))
        if "error" in response:
            raise _rpc_error_from_payload(response["error"], request_id=rid, method=method)
        return response.get("result")

    async def _rpc(
        self,
        method: str,
        params: dict[str, Any] | None = None,
        *,
        transport_timeout: float | None = None,
    ) -> Any:
        if self._transport is None:
            raise NotConnectedError("runtime not started — no transport available")
        rid = _next_request_id(method)
        request = _rpc_request(rid, method, params)
        if transport_timeout is None:
            response = await self._transport.send_async(request)
        else:
            response = await self._transport.send_async(
                request,
                timeout=transport_timeout,
            )
        if "error" in response:
            raise _rpc_error_from_payload(response["error"], request_id=rid, method=method)
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

    # -----------------------------------------------------------------
    # Identity-first runtime APIs (REQ-41)
    # -----------------------------------------------------------------

    def agent(self, identity: str) -> IdentityAgentHandle:
        """Return an identity-scoped agent handle."""
        return IdentityAgentHandle(self, identity)

    async def send(self, identity: str, content: "str | list") -> Any:
        """Send conversational content to an addressable identity."""
        from .identity_first_models import SendResult
        if isinstance(content, str):
            wire_content = content
        else:
            wire_content = [b.to_dict() for b in content]
        raw = await self._rpc("mobkit/send", {
            "identity": identity,
            "content": wire_content,
        })
        return SendResult.from_dict(raw) if isinstance(raw, dict) else raw

    async def dispatch(self, identity: str, dispatch_input: Any) -> Any:
        """Dispatch content to any identity (addressable or internal)."""
        from .identity_first_models import DispatchResult
        raw = await self._rpc("mobkit/dispatch", {
            "identity": identity,
            "dispatch_input": dispatch_input.to_dict(),
        })
        return DispatchResult.from_dict(raw) if isinstance(raw, dict) else raw

    async def dispatch_text(
        self,
        identity: str,
        text: str,
        *,
        origin: str = "system",
        correlation_id: str | None = None,
    ) -> Any:
        """Dispatch plain text without constructing DispatchInput manually."""
        from .identity_first_models import DispatchInput
        di = DispatchInput(content=text, origin=origin, correlation_id=correlation_id)
        return await self.dispatch(identity, di)

    async def subscribe(self, identity: str) -> Any:
        """Subscribe to identity-scoped events."""
        raw = await self._rpc("mobkit/subscribe", {"identity": identity})
        return raw

    async def status(self, identity: str) -> Any:
        """Return IdentityStatus for the given identity."""
        from .identity_first_models import IdentityStatus
        raw = await self._rpc("mobkit/status_identity", {"identity": identity})
        return IdentityStatus.from_dict(raw)

    async def inspect_identity(self, identity: str) -> Any:
        """Return execution-level inspection (output_preview, peer connectivity)."""
        from .identity_first_models import IdentityInspection
        raw = await self._rpc("mobkit/inspect_identity", {"identity": identity})
        return IdentityInspection.from_dict(raw)

    async def respawn(self, identity: str) -> Any:
        """Non-destructive durable recovery for an identity."""
        return await self._rpc("mobkit/respawn", {"identity": identity})

    async def retire(self, identity: str) -> Any:
        """Retire an identity through the standard retirement pipeline."""
        return await self._rpc("mobkit/retire", {"identity": identity})

    async def reset(self, identity: str) -> Any:
        """Destructive continuity reset for an identity."""
        return await self._rpc("mobkit/reset", {"identity": identity})

    async def reconcile(self) -> Any:
        """Re-run restore_flow with fresh roster from the provider.

        Picks up roster changes (added/removed/modified identities) and
        topology changes without restarting the runtime.
        """
        return await self._rpc("mobkit/reconcile_identity", {})

    async def identity_bootstrap_status(self) -> IdentityBootstrapStatus:
        """Return typed materialization status for the bootstrap roster."""
        raw = await self._rpc("mobkit/status_identity_bootstrap", {})
        return IdentityBootstrapStatus.from_dict(raw)

    async def wait_identity_bootstrap(
        self,
        *,
        timeout: float | None = None,
        target: str = "materialized",
    ) -> IdentityBootstrapStatus:
        """Wait for identity bootstrap materialization or startup readiness.

        ``target="materialized"`` waits for every roster identity to finish
        warming. ``target="startup_ready"`` additionally waits for the active
        members' startup-ready signals.
        """
        if target not in {"materialized", "startup_ready"}:
            raise ValueError("target must be 'materialized' or 'startup_ready'")
        if timeout is not None and (
            isinstance(timeout, bool)
            or not isinstance(timeout, (int, float))
            or not math.isfinite(timeout)
            or timeout < 0
        ):
            raise ValueError("timeout must be a non-negative finite number or None")
        params: dict[str, Any] = {"target": target}
        if self._transport is None:
            raise NotConnectedError("runtime not started — no transport available")
        server_wait_seconds = (
            self._transport.request_timeout if timeout is None else timeout
        )
        # Always make the server-side deadline explicit. Otherwise ``None``
        # inherits the gateway handler's shorter generic RPC timeout while the
        # SDK waits against its transport default, silently truncating the
        # readiness barrier.
        params["timeout_ms"] = int(server_wait_seconds * 1000)
        raw = await self._rpc(
            "mobkit/wait_identity_bootstrap",
            params,
            transport_timeout=(
                server_wait_seconds
                + _IDENTITY_BOOTSTRAP_WAIT_TRANSPORT_HEADROOM_SECONDS
            ),
        )
        return IdentityBootstrapStatus.from_dict(raw)

    async def delete_identity(self, identity: str) -> Any:
        """Remove continuity for an identity."""
        return await self._rpc("mobkit/delete_identity", {"identity": identity})

    async def wait_until_ready(
        self,
        identities: list[str],
        *,
        timeout: float = 60,
        poll_interval: float = 1.5,
    ) -> None:
        """Wait until all identities have completed their autonomous kickoff turn.

        Polls inspect_identity() until each identity has a non-None output_preview,
        meaning the kickoff turn has completed and the agent is ready for work.

        Note: readiness is inferred from output_preview presence — a proxy for
        kickoff completion. A future meerkat release may expose an explicit
        readiness signal, at which point this method will use that instead.
        """
        import time
        deadline = time.monotonic() + timeout
        remaining = set(identities)
        while remaining and time.monotonic() < deadline:
            for identity in list(remaining):
                try:
                    inspection = await self.inspect_identity(identity)
                    if inspection.output_preview is not None:
                        remaining.discard(identity)
                except Exception:
                    pass
            if remaining:
                await asyncio.sleep(poll_interval)
        if remaining:
            raise TimeoutError(
                f"identities did not become ready within {timeout}s: {sorted(remaining)}"
            )

    async def shutdown(self) -> None:
        async with self._lifecycle_lock:
            self._running = False
            shutdown_task = self._shutdown_task
            if shutdown_task is None:
                transport = self._transport
                # Close RPC admission before the potentially long process
                # drain. Every concurrent shutdown below observes and awaits
                # the same task; connect waits for it before replacing the
                # gateway.
                self._transport = None
                if transport is not None:
                    shutdown_task = self._schedule_transport_stop(transport)
        if shutdown_task is not None:
            # A cancelled caller must not cancel the runtime-owned cleanup.
            await asyncio.shield(shutdown_task)

    @property
    def is_running(self) -> bool:
        return self._running


class IdentityAgentHandle:
    """Identity-scoped agent handle for delivery, lifecycle, and observation."""

    def __init__(self, runtime: MobKitRuntime, identity: str):
        self._runtime = runtime
        self._identity = identity

    @property
    def identity(self) -> str:
        return self._identity

    async def send(self, content: Any) -> Any:
        """Send conversational content (Addressable only)."""
        return await self._runtime.send(self._identity, content)

    async def dispatch(self, dispatch_input: Any) -> Any:
        """Dispatch with a DispatchInput object."""
        return await self._runtime.dispatch(self._identity, dispatch_input)

    async def dispatch_text(
        self,
        text: str,
        *,
        origin: str = "system",
        correlation_id: str | None = None,
    ) -> Any:
        """Dispatch plain text without constructing DispatchInput."""
        return await self._runtime.dispatch_text(
            self._identity, text, origin=origin, correlation_id=correlation_id,
        )

    async def status(self) -> Any:
        """Return IdentityStatus."""
        return await self._runtime.status(self._identity)

    async def inspect(self) -> Any:
        """Return execution-level inspection (output_preview, peers)."""
        return await self._runtime.inspect_identity(self._identity)

    async def wait_until_ready(self, *, timeout: float = 60) -> None:
        """Wait until this identity's autonomous kickoff turn has completed."""
        await self._runtime.wait_until_ready(
            [self._identity], timeout=timeout,
        )

    async def wait_for_output(
        self,
        *,
        timeout: float = 90,
        poll_interval: float = 1.5,
        baseline: str | None = None,
    ) -> str:
        """Poll until this identity produces an output_preview.

        If baseline is given, waits until output_preview differs from it.
        Raises TimeoutError if timeout expires.
        """
        import time
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            inspection = await self.inspect()
            if inspection.output_preview:
                if baseline is None or inspection.output_preview != baseline:
                    return inspection.output_preview
            await asyncio.sleep(poll_interval)
        raise TimeoutError(
            f"identity {self._identity!r} did not produce output within {timeout}s"
        )

    async def wait_for_output_containing(
        self,
        needle: str,
        *,
        timeout: float = 90,
        poll_interval: float = 1.5,
    ) -> str:
        """Poll until output_preview contains the given substring."""
        import time
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            inspection = await self.inspect()
            if inspection.output_preview and needle in inspection.output_preview:
                return inspection.output_preview
            await asyncio.sleep(poll_interval)
        raise TimeoutError(
            f"identity {self._identity!r} did not produce output "
            f"containing {needle!r} within {timeout}s"
        )

    async def subscribe(self) -> Any:
        return await self._runtime.subscribe(self._identity)

    async def respawn(self) -> Any:
        return await self._runtime.respawn(self._identity)

    async def retire(self) -> Any:
        return await self._runtime.retire(self._identity)

    async def reset(self) -> Any:
        return await self._runtime.reset(self._identity)

    async def delete_identity(self) -> Any:
        return await self._runtime.delete_identity(self._identity)


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

    async def scheduling_evaluate(
        self,
        schedules: list[dict[str, Any]],
        tick_ms: int,
    ) -> Any:
        """Evaluate configured schedules at ``tick_ms``."""
        return await self._runtime._rpc(
            "mobkit/scheduling/evaluate",
            {"schedules": schedules, "tick_ms": tick_ms},
        )

    async def scheduling_dispatch(
        self,
        schedules: list[dict[str, Any]],
        tick_ms: int,
    ) -> Any:
        """Dispatch due schedules at ``tick_ms``."""
        return await self._runtime._rpc(
            "mobkit/scheduling/dispatch",
            {"schedules": schedules, "tick_ms": tick_ms},
        )

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

    async def memory_query(
        self,
        query: str | dict[str, Any] | None = None,
        **kwargs: Any,
    ) -> MemoryQueryResult:
        """Query the assertion ledger by entity/topic/store filters.

        Passing a string sends it as ``query``; the stock runtime applies it
        as a case-insensitive substring filter across entity, topic, and fact
        (reason for conflict signals), after the exact ``entity``/``topic``/
        ``store`` filters.
        """
        if isinstance(query, dict):
            params = {**query, **kwargs}
        elif query is None:
            params = {**kwargs}
        elif isinstance(query, str):
            params = {"query": query, **kwargs}
        else:
            raise TypeError("memory_query query must be a string, dict, or None")
        raw = await self._runtime._rpc("mobkit/memory/query", params)
        return MemoryQueryResult.from_dict(raw)

    async def remember_agent_memory(
        self,
        identity: str,
        *,
        title: str,
        body: str,
        tags: list[str] | None = None,
        realm: str | None = None,
    ) -> AgentMemoryRecord:
        """Persist a durable memory record for an identity-scoped agent."""
        params: dict[str, Any] = {
            "identity": identity,
            "title": title,
            "body": body,
        }
        if realm is not None:
            params["realm"] = realm
        if tags is not None:
            params["tags"] = list(tags)
        raw = await self._runtime._rpc("mobkit/agent_memory/remember", params)
        return AgentMemoryRecord.from_dict(raw)

    async def recall_agent_memory(
        self,
        identity: str,
        *,
        realm: str | None = None,
        selection: str | None = None,
        query_text: str | None = None,
        query_terms: list[str] | None = None,
        max_entries: int | None = None,
    ) -> list[AgentMemoryRecord]:
        """Recall durable memory records for an identity-scoped agent."""
        params: dict[str, Any] = {"identity": identity}
        if realm is not None:
            params["realm"] = realm
        if selection is not None:
            params["selection"] = selection
        if query_text is not None:
            params["query_text"] = query_text
        if query_terms is not None:
            params["query_terms"] = list(query_terms)
        if max_entries is not None:
            params["max_entries"] = max_entries
        raw = await self._runtime._rpc("mobkit/agent_memory/recall", params)
        return list(AgentMemoryRecallResult.from_dict(raw).records)

    async def forget_agent_memory(
        self,
        identity: str,
        memory_id: str,
        *,
        realm: str | None = None,
    ) -> AgentMemoryForgetResult:
        """Delete a durable memory record for an identity-scoped agent."""
        params: dict[str, Any] = {"identity": identity, "memory_id": memory_id}
        if realm is not None:
            params["realm"] = realm
        raw = await self._runtime._rpc("mobkit/agent_memory/forget", params)
        return AgentMemoryForgetResult.from_dict(raw)

    async def update_agent_memory(
        self,
        identity: str,
        memory_id: str,
        *,
        title: str,
        body: str,
        tags: list[str] | None = None,
        realm: str | None = None,
    ) -> AgentMemoryUpdateResult:
        """Supersede a durable memory record within its lineage.

        The new title/body/tags become the active record; the prior record
        stays retrievable with provenance and is no longer recalled.
        """
        params: dict[str, Any] = {
            "identity": identity,
            "memory_id": memory_id,
            "title": title,
            "body": body,
        }
        if realm is not None:
            params["realm"] = realm
        if tags is not None:
            params["tags"] = list(tags)
        raw = await self._runtime._rpc("mobkit/agent_memory/update", params)
        return AgentMemoryUpdateResult.from_dict(raw)

    async def manifest_agent_memory(
        self,
        identity: str,
        *,
        realm: str | None = None,
        tier: str | None = None,
        k: int | None = None,
    ) -> list[AgentMemoryRecordMeta]:
        """List durable memory record metadata (id/kind/title/description/
        age/rank — never bodies).

        ``tier`` is ``"working_set"`` (default; top-K ranked plus the
        recent/unranked slice) or ``"full"``; ``k`` bounds the working set.
        """
        params: dict[str, Any] = {"identity": identity}
        if realm is not None:
            params["realm"] = realm
        if tier is not None:
            params["tier"] = tier
        if k is not None:
            params["k"] = k
        raw = await self._runtime._rpc("mobkit/agent_memory/manifest", params)
        return list(AgentMemoryManifestResult.from_dict(raw).records)

    async def call_tool(
        self, module_id: str, tool: str, arguments: dict[str, Any] | None = None
    ) -> CallToolResult:
        """Call an MCP tool on a loaded module."""
        params: dict[str, Any] = {"module_id": module_id, "tool": tool}
        if arguments:
            params["arguments"] = arguments
        raw = await self._runtime._rpc("mobkit/call_tool", params)
        return CallToolResult.from_dict(raw)

    async def models_catalog(self) -> ModelsCatalogResult:
        """Return the curated model catalog with provider defaults."""
        raw = await self._runtime._rpc("mobkit/models/catalog")
        return ModelsCatalogResult.from_dict(raw)

    async def tools_catalog(self) -> MobpackToolsCatalogResult:
        """Return the MobKit tool catalog used by mobpack authoring."""
        raw = await self._runtime._rpc("mobkit/tools/catalog")
        return MobpackToolsCatalogResult.from_dict(raw)

    async def skills_catalog(self) -> MobpackSkillsCatalogResult:
        """Return the MobKit skill realms used by mobpack authoring."""
        raw = await self._runtime._rpc("mobkit/skills/catalog")
        return MobpackSkillsCatalogResult.from_dict(raw)

    async def agent_definitions(self) -> MobpackAgentDefinitionsResult:
        """Return MobKit-owned agent definitions for the Agent Editor."""
        raw = await self._runtime._rpc("mobkit/agent_definitions/list")
        return MobpackAgentDefinitionsResult.from_dict(raw)

    async def mobpack_templates(self) -> MobpackTemplatesResult:
        """Return blank and sample mobpack templates."""
        raw = await self._runtime._rpc("mobkit/mobpacks/templates")
        return MobpackTemplatesResult.from_dict(raw)

    async def mobpack_catalogs(self) -> MobpackCatalogsResult:
        """Return the composed MobKit mobpack authoring catalog snapshot."""
        raw = await self._runtime._rpc("mobkit/mobpacks/catalogs")
        return MobpackCatalogsResult.from_dict(raw)

    async def mobpack_validate(
        self,
        document: dict[str, Any],
        *,
        rkat_validate: bool | None = None,
    ) -> MobpackValidationResult:
        """Validate a mobpack authoring document."""
        params: dict[str, Any] = {"document": document}
        if rkat_validate is not None:
            params["rkat_validate"] = rkat_validate
        raw = await self._runtime._rpc("mobkit/mobpacks/validate", params)
        return MobpackValidationResult.from_dict(raw)

    async def mobpack_source(self, document: dict[str, Any]) -> MobpackSourceResult:
        """Render the deployable source files (mob.toml etc.) for a document."""
        raw = await self._runtime._rpc("mobkit/mobpacks/source", {"document": document})
        return MobpackSourceResult.from_dict(raw)

    async def mobpack_export(self, document: dict[str, Any]) -> MobpackExportResult:
        """Export a mobpack document as a base64-encoded archive."""
        raw = await self._runtime._rpc("mobkit/mobpacks/export", {"document": document})
        return MobpackExportResult.from_dict(raw)

    async def mobpack_import(
        self,
        *,
        mob_toml: str | None = None,
        content_base64: str | None = None,
        document: dict[str, Any] | None = None,
        source_name: str | None = None,
    ) -> MobpackImportResult:
        """Import a mob.toml, mobpack archive, or editor document."""
        params: dict[str, Any] = {}
        if mob_toml is not None:
            params["mob_toml"] = mob_toml
        if content_base64 is not None:
            params["content_base64"] = content_base64
        if document is not None:
            params["document"] = document
        if source_name is not None:
            params["source_name"] = source_name
        raw = await self._runtime._rpc("mobkit/mobpacks/import", params)
        return MobpackImportResult.from_dict(raw)

    async def mobpack_list(self) -> MobpackDraftListResult:
        """List mobpack draft registry rows."""
        raw = await self._runtime._rpc("mobkit/mobpacks/list", {})
        return MobpackDraftListResult.from_dict(raw)

    async def mobpack_get(self, draft_id: str) -> MobpackDraftGetResult:
        """Fetch a single mobpack draft registry row by id."""
        raw = await self._runtime._rpc("mobkit/mobpacks/get", {"id": draft_id})
        return MobpackDraftGetResult.from_dict(raw)

    async def mobpack_create(
        self,
        *,
        template: str | None = None,
        name: str | None = None,
        trigger: str | None = None,
    ) -> MobpackDraftSaveResult:
        """Create a new mobpack draft from a starter template."""
        params: dict[str, Any] = {}
        if template is not None:
            params["template"] = template
        if name is not None:
            params["name"] = name
        if trigger is not None:
            params["trigger"] = trigger
        raw = await self._runtime._rpc("mobkit/mobpacks/create", params)
        return MobpackDraftSaveResult.from_dict(raw)

    async def mobpack_save(
        self,
        draft_id: str,
        document: dict[str, Any],
        *,
        validation: dict[str, Any] | None = None,
        stage: str | None = None,
        expected_revision: int | None = None,
        expected_etag: str | None = None,
    ) -> MobpackDraftSaveResult:
        """Save a mobpack draft, optionally guarded by revision/etag."""
        params: dict[str, Any] = {"id": draft_id, "document": document}
        if validation is not None:
            params["validation"] = validation
        if stage is not None:
            params["stage"] = stage
        if expected_revision is not None:
            params["expected_revision"] = expected_revision
        if expected_etag is not None:
            params["expected_etag"] = expected_etag
        raw = await self._runtime._rpc("mobkit/mobpacks/save", params)
        return MobpackDraftSaveResult.from_dict(raw)

    async def mobpack_delete(
        self,
        draft_id: str,
        *,
        expected_revision: int | None = None,
    ) -> MobpackDraftDeleteResult:
        """Delete a mobpack draft, optionally guarded by revision."""
        params: dict[str, Any] = {"id": draft_id}
        if expected_revision is not None:
            params["expected_revision"] = expected_revision
        raw = await self._runtime._rpc("mobkit/mobpacks/delete", params)
        return MobpackDraftDeleteResult.from_dict(raw)

    async def mobpack_undo(
        self,
        draft_id: str,
        *,
        expected_revision: int | None = None,
        expected_etag: str | None = None,
    ) -> MobpackDraftHistoryResult:
        """Step a mobpack draft one entry back in its undo history."""
        params: dict[str, Any] = {"id": draft_id}
        if expected_revision is not None:
            params["expected_revision"] = expected_revision
        if expected_etag is not None:
            params["expected_etag"] = expected_etag
        raw = await self._runtime._rpc("mobkit/mobpacks/undo", params)
        return MobpackDraftHistoryResult.from_dict(raw)

    async def mobpack_redo(
        self,
        draft_id: str,
        *,
        expected_revision: int | None = None,
        expected_etag: str | None = None,
    ) -> MobpackDraftHistoryResult:
        """Step a mobpack draft one entry forward in its redo history."""
        params: dict[str, Any] = {"id": draft_id}
        if expected_revision is not None:
            params["expected_revision"] = expected_revision
        if expected_etag is not None:
            params["expected_etag"] = expected_etag
        raw = await self._runtime._rpc("mobkit/mobpacks/redo", params)
        return MobpackDraftHistoryResult.from_dict(raw)

    async def mobpack_apply_operation(
        self,
        document: dict[str, Any],
        operation: dict[str, Any],
        *,
        expected_catalog_snapshot_id: str | None = None,
    ) -> MobpackApplyOperationResult:
        """Apply a structured authoring operation to a mobpack document."""
        params: dict[str, Any] = {"document": document, "operation": operation}
        if expected_catalog_snapshot_id is not None:
            params["expected_catalog_snapshot_id"] = expected_catalog_snapshot_id
        raw = await self._runtime._rpc("mobkit/mobpacks/apply_operation", params)
        return MobpackApplyOperationResult.from_dict(raw)

    async def mobpack_deploy_command(
        self, document: dict[str, Any]
    ) -> MobpackDeployCommandResult:
        """Preview the ``rkat mob run`` deploy command for a mobpack document."""
        raw = await self._runtime._rpc(
            "mobkit/mobpacks/deploy_command", {"document": document}
        )
        return MobpackDeployCommandResult.from_dict(raw)

    async def mobpack_deploy(
        self,
        document: dict[str, Any],
        *,
        execute: bool | None = None,
    ) -> MobpackDeployResult:
        """Plan (and optionally execute) a mobpack deploy on the host."""
        params: dict[str, Any] = {"document": document}
        if execute is not None:
            params["execute"] = execute
        raw = await self._runtime._rpc("mobkit/mobpacks/deploy", params)
        return MobpackDeployResult.from_dict(raw)

    async def session_store_bigquery(self, **kwargs: Any) -> Any:
        """Run a BigQuery session-store RPC operation."""
        return await self._runtime._rpc("mobkit/session_store/bigquery", kwargs)

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
        self, member_id: str, role: str, **kwargs: Any
    ) -> MemberSnapshot:
        """Ensure a mob member exists, spawning it if missing.

        Idempotent — returns the member snapshot whether it was just spawned
        or already existed. Use before ``send()`` when handling first contact
        from an unknown user (e.g. new Slack DM).

        Args:
            member_id: Agent identity for the member.
            role: Role (profile name from mob.toml) to spawn with.
            **kwargs: Optional fields (labels, context, resume_session_id,
                      additional_instructions).
        """
        params: dict[str, Any] = {"role": role, "agent_identity": member_id}
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
                agent_identity = agents[0].agent_identity
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

    async def send(
        self,
        member_id: str,
        message: str | None = None,
        *,
        content: list[dict[str, Any]] | None = None,
        attachments: list[Any] | None = None,
        handling_mode: str | None = None,
    ) -> SendMessageResult:
        """Send a message to a mob member and return the accepting session.

        Args:
            member_id: Target member ID.
            message: Plain text message (simple path).
            content: Multimodal content blocks, e.g.
                ``[{"type": "text", "text": "describe this"},
                  {"type": "image", "media_type": "image/png",
                   "source": "inline", "data": "<base64>"}]``

        Either ``message`` or ``content`` must be provided. If both are given,
        ``content`` takes precedence so multimodal callers are not shadowed by
        stale text.
        """
        params: dict[str, Any] = {"member_id": member_id}
        if handling_mode is not None:
            params["handling_mode"] = handling_mode
        uploads = [_normalize_upload_item(item, idx) for idx, item in enumerate(attachments or [])]
        if content is not None:
            blocks: list[dict[str, Any]] = list(content)
        elif message is not None:
            blocks = [{"type": "text", "text": message}] if uploads else []
            if not uploads:
                params["message"] = message
        else:
            if not uploads:
                raise ValueError("message, content, or attachments must be provided")
            blocks = []
        if uploads:
            for idx, (_data, media_type, _filename) in enumerate(uploads):
                blocks.append(
                    {
                        "type": "image_upload",
                        "upload_id": f"upload-{idx + 1}",
                        "media_type": media_type,
                    }
                )
            params["content"] = blocks
            raw = await self._multipart_rpc("mobkit/send_message", params, uploads)
        else:
            raw = await self._runtime._rpc("mobkit/send_message", params)
        return SendMessageResult.from_dict(raw)

    async def get_blob(self, blob_id: str) -> dict[str, Any]:
        """Fetch a blob through the JSON-RPC compatibility boundary.

        The returned ``data`` field is base64 because JSON-RPC is not binary;
        MobKit stores and serves the blob internally as raw bytes.
        """
        raw = await self._runtime._rpc("mobkit/blob/get", {"blob_id": blob_id})
        return raw if isinstance(raw, dict) else {}

    async def upload_blob(
        self,
        file: bytes | bytearray | memoryview | str | Path,
        *,
        media_type: str | None = None,
        filename: str | None = None,
    ) -> dict[str, Any]:
        """Upload one image blob through the multipart console route.

        Returns ``{"blob_id", "media_type", "size"}``.
        """
        data, resolved_media_type, resolved_filename = _read_upload_source(
            file,
            media_type=media_type,
            filename=filename,
        )
        raw = await self._multipart_rpc(
            "mobkit/blob/upload",
            {
                "upload": {
                    "type": "image_upload",
                    "upload_id": "upload-1",
                    "media_type": resolved_media_type,
                }
            },
            [(data, resolved_media_type, resolved_filename)],
        )
        return raw if isinstance(raw, dict) else {}

    async def _multipart_rpc(
        self,
        method: str,
        params: dict[str, Any],
        uploads: list[tuple[bytes, str, str]],
    ) -> Any:
        base = self._runtime.rust_http_base_url
        if not base:
            raise NotConnectedError(
                "multipart RPC requires rust_http_base_url; start the gateway or call "
                "runtime.set_rust_http_base('http://127.0.0.1:8081')"
            )
        request_id = _next_request_id(method)
        payload = _rpc_request(request_id, method, params)
        boundary = f"mobkit-{uuid.uuid4().hex}"
        body = self._encode_multipart_body(boundary, payload, uploads)
        req = urllib_request.Request(
            base.rstrip("/") + "/console/rpc/multipart",
            data=body,
            method="POST",
            headers={
                "content-type": f"multipart/form-data; boundary={boundary}",
                "accept": "application/json",
            },
        )
        try:
            response_text = await asyncio.to_thread(self._post_multipart_request, req)
        except HTTPError as exc:
            response_text = exc.read().decode("utf-8", errors="replace")
            raise TransportError(
                f"multipart RPC failed (status={exc.code}): {response_text}"
            ) from exc
        except URLError as exc:
            raise TransportError(f"multipart RPC failed: {exc.reason}") from exc
        response = json.loads(response_text)
        if "error" in response:
            raise _rpc_error_from_payload(
                response["error"], request_id=request_id, method=method
            )
        return response.get("result")

    @staticmethod
    def _encode_multipart_body(
        boundary: str,
        payload: dict[str, Any],
        uploads: list[tuple[bytes, str, str]],
    ) -> bytes:
        lines: list[bytes] = []

        def add(value: str | bytes) -> None:
            lines.append(value if isinstance(value, bytes) else value.encode("utf-8"))

        add(f"--{boundary}\r\n")
        add('Content-Disposition: form-data; name="payload"\r\n')
        add("Content-Type: application/json\r\n\r\n")
        add(json.dumps(payload))
        add("\r\n")
        for idx, (data, media_type, filename) in enumerate(uploads):
            upload_id = f"upload-{idx + 1}"
            safe_filename = filename.replace('"', "")
            add(f"--{boundary}\r\n")
            add(
                f'Content-Disposition: form-data; name="file:{upload_id}"; '
                f'filename="{safe_filename}"\r\n'
            )
            add(f"Content-Type: {media_type}\r\n\r\n")
            add(data)
            add("\r\n")
        add(f"--{boundary}--\r\n")
        return b"".join(lines)

    @staticmethod
    def _post_multipart_request(req: urllib_request.Request) -> str:
        with urllib_request.urlopen(req, timeout=60) as response:
            return response.read().decode("utf-8")

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
        events = raw
        if isinstance(raw, dict) and raw.get("status") == "no_event_log_configured":
            events = raw.get("events", [])
        if isinstance(events, list):
            return [PersistedEvent.from_dict(e) for e in events]
        return []

    async def query_mob_events(
        self,
        query: "EventQuery | dict[str, Any] | None" = None,
    ) -> list[MobStructuralEvent]:
        """Query structural mob events from the meerkat ledger.

        Returns events matching ``query``. Pass the highest seen
        ``cursor`` as ``EventQuery.after_seq`` to paginate. Without
        ``after_seq`` the call returns the latest matching events
        (default ``limit = 256``).

        Raises :class:`MobEventsStaleError` when ``after_seq`` is past
        the current ledger frontier; the exception carries
        ``after_cursor`` and ``latest_cursor`` so callers can rewind.
        """
        from .types import EventQuery, MobStructuralEvent
        if query is None:
            params: dict[str, Any] = {}
        elif isinstance(query, EventQuery):
            params = query.to_dict()
        elif isinstance(query, dict):
            params = dict(query)
        else:
            raise TypeError(f"unsupported query type: {type(query).__name__}")
        try:
            raw = await self._runtime._rpc("mobkit/mob_events/query", params)
        except RpcError as err:
            if err.code == MOB_EVENTS_STALE_CURSOR_CODE:
                raise MobEventsStaleError.from_rpc_error(err) from err
            raise
        events: list[Any] = []
        if isinstance(raw, dict):
            maybe = raw.get("events")
            if isinstance(maybe, list):
                events = maybe
        elif isinstance(raw, list):
            events = raw
        return [MobStructuralEvent.from_dict(e) for e in events if isinstance(e, dict)]

    async def subscribe_mob_events(
        self,
        query: "EventQuery | dict[str, Any] | None" = None,
    ) -> AsyncIterator[MobStructuralEvent]:
        """Replay structural mob events as an async iterator.

        Returns an async iterator over the snapshot frame returned by
        ``mobkit/mob_events/subscribe``. Live tailing for production
        streaming uses the SSE bridge at ``/mobkit/mob_events/stream``;
        this method is the JSON-RPC snapshot equivalent of
        :meth:`query_mob_events` for clients that prefer an iterator.

        Raises :class:`MobEventsStaleError` when ``after_seq`` is past
        the current ledger frontier.
        """
        from .types import EventQuery, MobStructuralEvent
        if query is None:
            params: dict[str, Any] = {}
        elif isinstance(query, EventQuery):
            params = query.to_dict()
        elif isinstance(query, dict):
            params = dict(query)
        else:
            raise TypeError(f"unsupported query type: {type(query).__name__}")
        try:
            raw = await self._runtime._rpc("mobkit/mob_events/subscribe", params)
        except RpcError as err:
            if err.code == MOB_EVENTS_STALE_CURSOR_CODE:
                raise MobEventsStaleError.from_rpc_error(err) from err
            raise
        events: list[Any] = []
        if isinstance(raw, dict):
            maybe = raw.get("events")
            if isinstance(maybe, list):
                events = maybe
        elif isinstance(raw, list):
            # Wire shape parity with `query_mob_events`: a server that
            # returns a bare list (older gateway, future shape change)
            # must still yield events instead of silently dropping them.
            events = raw
        for entry in events:
            if isinstance(entry, dict):
                yield MobStructuralEvent.from_dict(entry)

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

    # -----------------------------------------------------------------
    # WorkGraph — collaborative work-item graph
    # -----------------------------------------------------------------

    async def workgraph_snapshot(self, **kwargs: Any) -> WorkGraphSnapshotResult:
        """Return a full WorkGraph snapshot (items, edges, attention, ready ids)."""
        raw = await self._runtime._rpc("mobkit/workgraph/snapshot", dict(kwargs))
        return WorkGraphSnapshotResult.from_dict(raw)

    async def workgraph_list(self, **kwargs: Any) -> WorkGraphItemsResult:
        """List WorkGraph items matching the given filter."""
        raw = await self._runtime._rpc("mobkit/workgraph/list", dict(kwargs))
        return WorkGraphItemsResult.from_dict(raw)

    async def workgraph_get(self, id: str, **kwargs: Any) -> WorkGraphItem:
        """Fetch a single WorkGraph item by id."""
        params: dict[str, Any] = {"id": id, **kwargs}
        raw = await self._runtime._rpc("mobkit/workgraph/get", params)
        item_data = raw.get("item", raw) if isinstance(raw, dict) else raw
        return WorkGraphItem.from_dict(item_data)

    async def workgraph_ready(self, **kwargs: Any) -> WorkGraphItemsResult:
        """List WorkGraph items that are ready to claim (no unresolved blockers)."""
        raw = await self._runtime._rpc("mobkit/workgraph/ready", dict(kwargs))
        return WorkGraphItemsResult.from_dict(raw)

    async def workgraph_events(self, **kwargs: Any) -> list[WorkGraphEventEntry]:
        """Query the WorkGraph event log."""
        raw = await self._runtime._rpc("mobkit/workgraph/events", dict(kwargs))
        events = raw.get("events", []) if isinstance(raw, dict) else []
        return [WorkGraphEventEntry.from_dict(e) for e in events]

    async def workgraph_attention_list(
        self, **kwargs: Any
    ) -> list[WorkGraphAttentionBinding]:
        """List attention bindings matching the given filter."""
        raw = await self._runtime._rpc("mobkit/workgraph/attention/list", dict(kwargs))
        bindings = raw.get("attention", []) if isinstance(raw, dict) else []
        return [WorkGraphAttentionBinding.from_dict(b) for b in bindings]

    async def workgraph_goal_status(
        self, binding_id: str, **kwargs: Any
    ) -> WorkGraphGoalResult:
        """Fetch the goal work item + attention binding for a binding id."""
        params: dict[str, Any] = {"binding_id": binding_id, **kwargs}
        raw = await self._runtime._rpc("mobkit/workgraph/goal/status", params)
        return WorkGraphGoalResult.from_dict(raw)

    async def workgraph_create(self, title: str, **kwargs: Any) -> WorkGraphItem:
        """Create a new WorkGraph item."""
        params: dict[str, Any] = {"title": title, **kwargs}
        raw = await self._runtime._rpc("mobkit/workgraph/create", params)
        item_data = raw.get("item", raw) if isinstance(raw, dict) else raw
        return WorkGraphItem.from_dict(item_data)

    async def workgraph_update(
        self, id: str, expected_revision: int, **kwargs: Any
    ) -> WorkGraphItem:
        """Update a WorkGraph item's mutable fields (CAS via ``expected_revision``)."""
        params: dict[str, Any] = {
            "id": id,
            "expected_revision": expected_revision,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/update", params)
        item_data = raw.get("item", raw) if isinstance(raw, dict) else raw
        return WorkGraphItem.from_dict(item_data)

    async def workgraph_claim(
        self,
        id: str,
        expected_revision: int,
        owner: dict[str, Any],
        **kwargs: Any,
    ) -> WorkGraphItem:
        """Claim a WorkGraph item for an owner (CAS via ``expected_revision``)."""
        params: dict[str, Any] = {
            "id": id,
            "expected_revision": expected_revision,
            "owner": owner,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/claim", params)
        item_data = raw.get("item", raw) if isinstance(raw, dict) else raw
        return WorkGraphItem.from_dict(item_data)

    async def workgraph_release(
        self, id: str, expected_revision: int, **kwargs: Any
    ) -> WorkGraphItem:
        """Release a claimed WorkGraph item (CAS via ``expected_revision``)."""
        params: dict[str, Any] = {
            "id": id,
            "expected_revision": expected_revision,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/release", params)
        item_data = raw.get("item", raw) if isinstance(raw, dict) else raw
        return WorkGraphItem.from_dict(item_data)

    async def workgraph_close(
        self, id: str, expected_revision: int, **kwargs: Any
    ) -> WorkGraphItem:
        """Close a WorkGraph item (default status ``completed``)."""
        params: dict[str, Any] = {
            "id": id,
            "expected_revision": expected_revision,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/close", params)
        item_data = raw.get("item", raw) if isinstance(raw, dict) else raw
        return WorkGraphItem.from_dict(item_data)

    async def workgraph_block(
        self, id: str, expected_revision: int, **kwargs: Any
    ) -> WorkGraphItem:
        """Mark a WorkGraph item blocked (CAS via ``expected_revision``)."""
        params: dict[str, Any] = {
            "id": id,
            "expected_revision": expected_revision,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/block", params)
        item_data = raw.get("item", raw) if isinstance(raw, dict) else raw
        return WorkGraphItem.from_dict(item_data)

    async def workgraph_link(
        self, kind: str, from_id: str, to_id: str, **kwargs: Any
    ) -> WorkGraphEdge:
        """Link two WorkGraph items with an edge (e.g. ``blocks``, ``parent``)."""
        params: dict[str, Any] = {
            "kind": kind,
            "from_id": from_id,
            "to_id": to_id,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/link", params)
        edge_data = raw.get("edge", raw) if isinstance(raw, dict) else raw
        return WorkGraphEdge.from_dict(edge_data)

    async def workgraph_add_evidence(
        self,
        id: str,
        expected_revision: int,
        evidence: dict[str, Any],
        **kwargs: Any,
    ) -> WorkGraphItem:
        """Attach an evidence reference to a WorkGraph item."""
        params: dict[str, Any] = {
            "id": id,
            "expected_revision": expected_revision,
            "evidence": evidence,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/evidence/add", params)
        item_data = raw.get("item", raw) if isinstance(raw, dict) else raw
        return WorkGraphItem.from_dict(item_data)

    async def workgraph_escalate_policy(
        self,
        binding_id: str,
        id: str,
        expected_revision: int,
        completion_policy: dict[str, Any],
        **kwargs: Any,
    ) -> WorkGraphItem:
        """Escalate a WorkGraph item's completion policy under a goal's authority.

        The witness (``AttentionContextProjection``) is fetched server-side
        from ``binding_id`` — the SDK does not build or forward it.
        """
        params: dict[str, Any] = {
            "binding_id": binding_id,
            "id": id,
            "expected_revision": expected_revision,
            "completion_policy": completion_policy,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/policy/escalate", params)
        item_data = raw.get("item", raw) if isinstance(raw, dict) else raw
        return WorkGraphItem.from_dict(item_data)

    async def workgraph_goal_create(
        self, title: str, target: dict[str, Any], **kwargs: Any
    ) -> WorkGraphGoalResult:
        """Create a goal work item plus its attention binding.

        ``target`` is ``{"kind": "session", "session_id": ...}``,
        ``{"kind": "identity", "identity": ...}`` (lowered server-side to an
        owner key against the runtime's mob id), or
        ``{"kind": "owner", "owner_key": {"kind": ..., "id": ...}}``.
        """
        params: dict[str, Any] = {"title": title, "target": target, **kwargs}
        raw = await self._runtime._rpc("mobkit/workgraph/goal/create", params)
        return WorkGraphGoalResult.from_dict(raw)

    async def workgraph_goal_confirm(
        self, binding_id: str, expected_revision: int, **kwargs: Any
    ) -> WorkGraphGoalResult:
        """Confirm a goal's completion (CAS via ``expected_revision``)."""
        params: dict[str, Any] = {
            "binding_id": binding_id,
            "expected_revision": expected_revision,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/goal/confirm", params)
        return WorkGraphGoalResult.from_dict(raw)

    async def workgraph_goal_request_close(
        self, binding_id: str, expected_revision: int, **kwargs: Any
    ) -> WorkGraphGoalResult:
        """Request closure of a goal (default status ``completed``)."""
        params: dict[str, Any] = {
            "binding_id": binding_id,
            "expected_revision": expected_revision,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/goal/request_close", params)
        return WorkGraphGoalResult.from_dict(raw)

    async def workgraph_attention_pause(
        self, binding_id: str, expected_revision: int, **kwargs: Any
    ) -> WorkGraphAttentionBinding:
        """Pause an attention binding (optionally until a given time)."""
        params: dict[str, Any] = {
            "binding_id": binding_id,
            "expected_revision": expected_revision,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/attention/pause", params)
        binding_data = raw.get("attention", raw) if isinstance(raw, dict) else raw
        return WorkGraphAttentionBinding.from_dict(binding_data)

    async def workgraph_attention_resume(
        self, binding_id: str, expected_revision: int, **kwargs: Any
    ) -> WorkGraphAttentionBinding:
        """Resume a paused attention binding."""
        params: dict[str, Any] = {
            "binding_id": binding_id,
            "expected_revision": expected_revision,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/attention/resume", params)
        binding_data = raw.get("attention", raw) if isinstance(raw, dict) else raw
        return WorkGraphAttentionBinding.from_dict(binding_data)

    async def workgraph_attention_reassign(
        self,
        binding_id: str,
        expected_revision: int,
        target: dict[str, Any],
        **kwargs: Any,
    ) -> WorkGraphAttentionReassignResult:
        """Reassign an attention binding to a new target.

        The witness is fetched server-side from ``binding_id``, mirroring
        ``workgraph_escalate_policy``.
        """
        params: dict[str, Any] = {
            "binding_id": binding_id,
            "expected_revision": expected_revision,
            "target": target,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/workgraph/attention/reassign", params)
        return WorkGraphAttentionReassignResult.from_dict(raw)

    async def workgraph_attention_prune(
        self, updated_before: str | None = None, **kwargs: Any
    ) -> int:
        """Prune TERMINAL (superseded/stopped) attention binding rows.

        The workgraph event stream keeps the audit history; binding rows
        otherwise grow monotonically with reassignment churn.  Pass an
        RFC3339 ``updated_before`` to prune only rows last updated strictly
        before that instant.  Returns the number of rows pruned.
        """
        params: dict[str, Any] = dict(kwargs)
        if updated_before is not None:
            params["updated_before"] = updated_before
        raw = await self._runtime._rpc("mobkit/workgraph/attention/prune", params)
        pruned = raw.get("pruned", 0) if isinstance(raw, dict) else 0
        return int(pruned)

    # -----------------------------------------------------------------
    # Live (realtime) sessions — mobkit/live/* (mobkit 0.7.31)
    # -----------------------------------------------------------------

    async def live_open(
        self, identity: str, **kwargs: Any
    ) -> dict[str, Any]:
        """Open a realtime (live) channel on a member's session.

        Returns the transport bootstrap::

            {"channel_id": ..., "transport": {"type": "websocket",
             "url": ..., "token": ...}, "capabilities": {...},
             "continuity": {...}}

        The token is single-use with a short TTL — hand the URL to the
        client (robot, satellite process) immediately.  Optional kwargs are
        forwarded (e.g. ``model=`` to override the realtime model,
        ``turning_mode=``).
        """
        params: dict[str, Any] = {"identity": identity, **kwargs}
        raw = await self._runtime._rpc("mobkit/live/open", params)
        return raw if isinstance(raw, dict) else {}

    async def live_status(self, identity: str, **kwargs: Any) -> dict[str, Any]:
        """Status of the member's live channel (open/closed, channel id)."""
        params: dict[str, Any] = {"identity": identity, **kwargs}
        raw = await self._runtime._rpc("mobkit/live/status", params)
        return raw if isinstance(raw, dict) else {}

    async def live_close(self, identity: str, **kwargs: Any) -> dict[str, Any]:
        """Close the member's live channel."""
        params: dict[str, Any] = {"identity": identity, **kwargs}
        raw = await self._runtime._rpc("mobkit/live/close", params)
        return raw if isinstance(raw, dict) else {}

    async def live_send_input_image(
        self,
        identity: str,
        idempotency_key: str,
        mime: str,
        data_base64: str,
        **kwargs: Any,
    ) -> dict[str, Any]:
        """Send a still image into the member's open live channel (meerkat
        0.7.27+).  ``idempotency_key`` must be caller-stable within the
        session — retries with the same key are exact-retry deduplicated.
        """
        params: dict[str, Any] = {
            "identity": identity,
            "chunk": {
                "kind": "image",
                "idempotency_key": idempotency_key,
                "mime": mime,
                "data": data_base64,
            },
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/live/send_input", params)
        return raw if isinstance(raw, dict) else {}

    async def live_refresh(self, identity: str, **kwargs: Any) -> dict[str, Any]:
        """Push refreshed mutable config (instructions/tools/audio) into an
        open live channel without rebuilding the transport.  Model/provider
        swaps require close + reopen."""
        params: dict[str, Any] = {"identity": identity, **kwargs}
        raw = await self._runtime._rpc("mobkit/live/refresh", params)
        return raw if isinstance(raw, dict) else {}

    async def live_truncate(
        self,
        channel_id: str,
        item_id: str,
        content_index: int,
        audio_played_ms: int,
        **kwargs: Any,
    ) -> dict[str, Any]:
        """Truncate an assistant item at the client-tracked playback cursor
        (barge-in cleanup).  ``item_id``/``content_index`` are the
        provider-side handle for the assistant item; ``audio_played_ms`` is
        how much the client actually played."""
        params: dict[str, Any] = {
            "channel_id": channel_id,
            "item_id": item_id,
            "content_index": content_index,
            "audio_played_ms": audio_played_ms,
            **kwargs,
        }
        raw = await self._runtime._rpc("mobkit/live/truncate", params)
        return raw if isinstance(raw, dict) else {}

    # -----------------------------------------------------------------
    # Cross-mob operations
    # -----------------------------------------------------------------

    async def wire_cross_mob(
        self,
        local_member_id: str,
        remote_member_id: str,
        remote_handle: "MobHandle",
    ) -> None:
        """Wire a local member to a member on another mob handle.

        Uses peer_info + wire_local on both sides.  Both mob handles must
        live in the **same process** — ``peer_info()`` always returns an
        ``inproc://`` address, which is only reachable within the same
        process.  For cross-process peering, call ``wire_local()`` on each
        handle directly, supplying the remote gateway's routable TCP or UDS
        address in place of the inproc address from ``peer_info()``.

        Args:
            local_member_id: Member on this mob to wire.
            remote_member_id: Member on the remote mob to wire.
            remote_handle: MobHandle for the remote mob (must be same-process).
        """
        local_info = await self.peer_info(local_member_id)
        remote_info = await remote_handle.peer_info(remote_member_id)
        await self.wire_local(
            local_member_id,
            remote_info["comms_name"],
            remote_info["peer_id"],
            remote_info["address"],
        )
        try:
            await remote_handle.wire_local(
                remote_member_id,
                local_info["comms_name"],
                local_info["peer_id"],
                local_info["address"],
            )
        except Exception:
            # Best-effort rollback: undo the local wire using the same
            # low-level API — no contact directory or peer handles needed.
            try:
                await self.unwire_local(
                    local_member_id,
                    remote_info["comms_name"],
                    remote_info["peer_id"],
                    remote_info["address"],
                )
            except Exception:
                pass
            raise

    async def send_cross_mob(
        self,
        remote_member_id: str,
        remote_handle: "MobHandle",
        message: str | None = None,
        *,
        content: list[dict[str, Any]] | None = None,
    ) -> SendMessageResult:
        """Send a message to a member on another mob handle.

        This is an app-level injection — the remote agent receives the
        message but does not know the sender. For agent-to-agent
        communication with sender identity and reply path, use
        ``wire_cross_mob()`` to set up peering, then agents communicate
        directly via their comms ``send`` tool.

        Args:
            remote_member_id: Target member on the remote mob.
            remote_handle: MobHandle for the remote mob.
            message: Plain text message.
            content: Multimodal content blocks.
        """
        return await remote_handle.send(remote_member_id, message, content=content)

    async def list_external_mobs(self) -> list:
        """List known external mobs from the contact directory."""
        from .types import CrossMobContactEntry

        raw = await self._runtime._rpc("mobkit/cross_mob/directory")
        mobs = raw.get("mobs", []) if isinstance(raw, dict) else []
        return [CrossMobContactEntry.from_dict(m) for m in mobs]

    async def peer_info(self, member_id: str) -> dict[str, str]:
        """Get comms peer info for a local member.

        Returns ``{"member_id", "mob_id", "comms_name", "peer_id", "address"}``.
        Used to build peer specs for cross-mob wiring via ``wire_local``.
        """
        raw = await self._runtime._rpc(
            "mobkit/cross_mob/peer_info",
            {"member_id": member_id},
        )
        return raw if isinstance(raw, dict) else {}

    async def peer_pubkey(self) -> str:
        """Return the local gateway's Ed25519 signing pubkey, base64.

        Used to bootstrap trust before populating a peer mobkit's
        contact directory with this gateway's pubkey. Raises
        :class:`CapabilityUnavailableError` from the underlying
        transport if the local gateway is inproc-only and never
        configured a keypair.
        """
        raw = await self._runtime._rpc("mobkit/peer_pubkey")
        if isinstance(raw, dict):
            value = raw.get("pubkey_b64")
            if isinstance(value, str):
                return value
        return ""

    async def wire_local(
        self,
        local_member_id: str,
        remote_comms_name: str,
        remote_peer_id: str,
        remote_address: str,
        *,
        remote_pubkey_b64: str | None = None,
    ) -> None:
        """Wire a local member to a remote peer (local side only).

        For same-process (inproc) cross-mob wiring::

            # Get peer info from each side
            a_info = await core.peer_info("school")
            b_info = await gw.peer_info("calendar")

            # Wire each side to the other (inproc address from peer_info)
            await core.wire_local("school", b_info["comms_name"], b_info["peer_id"], b_info["address"])
            await gw.wire_local("calendar", a_info["comms_name"], a_info["peer_id"], a_info["address"])

        For cross-process (TCP/UDS), replace the address with the remote
        gateway's transport endpoint — peer_info always returns inproc.
        Pass ``remote_pubkey_b64`` (base64 of the peer gateway's Ed25519
        verifying key, fetched via :meth:`peer_pubkey`) to stamp a real
        signing pubkey on the descriptor; the gateway rejects non-inproc
        wires without one.
        """
        params: dict[str, str] = {
            "local_member_id": local_member_id,
            "remote_comms_name": remote_comms_name,
            "remote_peer_id": remote_peer_id,
            "remote_address": remote_address,
        }
        if remote_pubkey_b64 is not None:
            params["remote_pubkey_b64"] = remote_pubkey_b64
        await self._runtime._rpc("mobkit/cross_mob/wire_local", params)

    async def unwire_local(
        self,
        local_member_id: str,
        remote_comms_name: str,
        remote_peer_id: str,
        remote_address: str,
        *,
        remote_pubkey_b64: str | None = None,
    ) -> None:
        """Undo a wire_local — unwire a local member from a previously wired peer (local side only)."""
        params: dict[str, str] = {
            "local_member_id": local_member_id,
            "remote_comms_name": remote_comms_name,
            "remote_peer_id": remote_peer_id,
            "remote_address": remote_address,
        }
        if remote_pubkey_b64 is not None:
            params["remote_pubkey_b64"] = remote_pubkey_b64
        await self._runtime._rpc("mobkit/cross_mob/unwire_local", params)

    # -----------------------------------------------------------------
    # Rich member inspection
    # -----------------------------------------------------------------

    async def member_status(self, member_id: str) -> RichMemberSnapshot:
        """Return rich execution status for a member."""
        from .types import RichMemberSnapshot
        raw = await self._runtime._rpc("mobkit/member_status", {"member_id": member_id})
        return RichMemberSnapshot.from_dict(raw)

    async def identity_resolved_tools(self, identity: str) -> list[str]:
        """Return the tools currently visible to an identity's live session."""
        raw = await self._runtime._rpc(
            "mobkit/identity/resolved_tools",
            {"identity": identity},
        )
        return [str(tool) for tool in raw.get("tools", [])]

    async def identity_resolved_tools_detail(self, identity: str):
        """Return the full resolved-tools diagnostic payload for an identity."""
        from .types import IdentityResolvedToolsResult
        raw = await self._runtime._rpc(
            "mobkit/identity/resolved_tools",
            {"identity": identity},
        )
        return IdentityResolvedToolsResult.from_dict(raw)

    async def force_cancel_member(self, member_id: str) -> None:
        """Force-cancel a running member immediately."""
        await self._runtime._rpc("mobkit/force_cancel_member", {"member_id": member_id})

    # -----------------------------------------------------------------
    # Helper convenience
    # -----------------------------------------------------------------

    async def spawn_helper(
        self,
        agent_identity: str,
        task: str,
        *,
        role: str | None = None,
        runtime_mode: str | None = None,
        backend: str | None = None,
    ) -> HelperResult:
        """Spawn a short-lived helper member and return its result."""
        from .types import HelperResult
        params: dict[str, Any] = {"agent_identity": agent_identity, "task": task}
        options: dict[str, Any] = {}
        if role is not None:
            options["role"] = role
        if runtime_mode is not None:
            options["runtime_mode"] = runtime_mode
        if backend is not None:
            options["backend"] = backend
        if options:
            params["options"] = options
        raw = await self._runtime._rpc("mobkit/spawn_helper", params)
        return HelperResult.from_dict(raw)

    async def fork_helper(
        self,
        source_member_id: str,
        agent_identity: str,
        task: str,
        *,
        fork_context: dict | None = None,
        role: str | None = None,
        runtime_mode: str | None = None,
        backend: str | None = None,
    ) -> HelperResult:
        """Fork a helper from an existing member's context."""
        from .types import HelperResult
        params: dict[str, Any] = {
            "source_member_id": source_member_id,
            "agent_identity": agent_identity,
            "task": task,
        }
        if fork_context is not None:
            params["fork_context"] = fork_context
        options: dict[str, Any] = {}
        if role is not None:
            options["role"] = role
        if runtime_mode is not None:
            options["runtime_mode"] = runtime_mode
        if backend is not None:
            options["backend"] = backend
        if options:
            params["options"] = options
        raw = await self._runtime._rpc("mobkit/fork_helper", params)
        return HelperResult.from_dict(raw)

    # -----------------------------------------------------------------
    # Session attachment
    # -----------------------------------------------------------------

    async def attach_session(
        self,
        role: str,
        agent_identity: str,
        session_id: str,
    ) -> RichMemberSnapshot:
        """Attach a member to an existing session (resume mode)."""
        from .types import RichMemberSnapshot
        params: dict[str, Any] = {
            "role": role,
            "agent_identity": agent_identity,
            "session_id": session_id,
        }
        raw = await self._runtime._rpc("mobkit/attach_existing_session", params)
        return RichMemberSnapshot.from_dict(raw)

    # -----------------------------------------------------------------
    # Flow lifecycle
    # -----------------------------------------------------------------

    async def cancel_flow(self, run_id: str) -> None:
        """Cancel a running flow by run ID."""
        await self._runtime._rpc("mobkit/cancel_flow", {"run_id": run_id})

    async def flow_status(self, run_id: str) -> MobRunSnapshot | None:
        """Get flow run status. Returns None if run not found."""
        from .types import MobRunSnapshot
        raw = await self._runtime._rpc("mobkit/flow_status", {"run_id": run_id})
        if raw is None:
            return None
        if isinstance(raw, dict) and raw.get("status") == "not_found":
            return None
        return MobRunSnapshot.from_dict(raw)

    async def list_flows(self) -> list[str]:
        """List all configured flow IDs in this mob definition.

        Relays meerkat 0.6's ``MobHandle::list_flows``. Returns the flow IDs
        declared by the mob's ``[flows.*]`` tables, in unspecified order.
        """
        raw = await self._runtime._rpc("mobkit/list_flows")
        if isinstance(raw, dict):
            flows = raw.get("flows", [])
        elif isinstance(raw, list):
            flows = raw
        else:
            flows = []
        return [str(flow_id) for flow_id in flows]

    async def list_runs(self, flow_id: str | None = None) -> list[MobRun]:
        """List flow runs for this mob.

        Relays ``MobHandle::list_runs``. When ``flow_id`` is given,
        only runs for that flow are returned. The returned :class:`MobRun`
        carries the full meerkat ledger projection — ``step_ledger``,
        ``failure_ledger``, ``frames``, ``loops``, ``loop_iteration_ledger``,
        ``flow_state``, ``activation_params``, etc. — verbatim from the
        on-the-wire JSON.
        """
        from .types import MobRun
        params: dict[str, Any] = {}
        if flow_id is not None:
            params["flow_id"] = flow_id
        raw = await self._runtime._rpc("mobkit/list_runs", params)
        runs: list[Any] = []
        if isinstance(raw, dict):
            maybe = raw.get("runs")
            if isinstance(maybe, list):
                runs = maybe
        elif isinstance(raw, list):
            runs = raw
        return [MobRun.from_dict(r) for r in runs if isinstance(r, dict)]

    async def run_flow(self, flow_id: str, params: Any = None) -> str:
        """Start a flow run and return its run ID.

        Relays meerkat 0.6's ``MobHandle::run_flow``. ``params`` is forwarded
        verbatim as the flow's activation params (any JSON value, defaults to
        ``None``). The returned ``run_id`` can be passed to
        :meth:`flow_status` and :meth:`cancel_flow`.
        """
        rpc_params: dict[str, Any] = {"flow_id": flow_id, "params": params}
        raw = await self._runtime._rpc("mobkit/run_flow", rpc_params)
        if isinstance(raw, dict):
            run_id = raw.get("run_id")
            if isinstance(run_id, str):
                return run_id
        raise RuntimeError(f"unexpected run_flow response: {raw!r}")

    # -----------------------------------------------------------------
    # Mob/run labels — mobkit-side sidecar metadata
    # -----------------------------------------------------------------

    async def set_mob_labels(self, labels: dict[str, str]) -> None:
        """Replace the label set associated with this mob.

        Mobkit owns these labels — they are not part of meerkat-mob.
        Replacement is wholesale; existing labels not present in
        ``labels`` are dropped. Pass ``{}`` to clear.
        """
        await self._runtime._rpc("mobkit/mob_labels/set", {"labels": dict(labels)})

    async def get_mob_labels(self) -> dict[str, str]:
        """Return the label set associated with this mob (or ``{}``)."""
        raw = await self._runtime._rpc("mobkit/mob_labels/get")
        if isinstance(raw, dict):
            labels = raw.get("labels", {})
            if isinstance(labels, dict):
                return {str(k): str(v) for k, v in labels.items()}
        return {}

    async def delete_mob_labels(self) -> None:
        """Remove the label set associated with this mob."""
        await self._runtime._rpc("mobkit/mob_labels/delete")

    async def set_run_labels(self, run_id: str, labels: dict[str, str]) -> None:
        """Replace the label set associated with ``run_id`` under this mob.

        Replacement is wholesale (see :meth:`set_mob_labels`).
        """
        await self._runtime._rpc(
            "mobkit/run_labels/set", {"run_id": run_id, "labels": dict(labels)}
        )

    async def get_run_labels(self, run_id: str) -> dict[str, str]:
        """Return the label set for ``run_id`` (or ``{}``)."""
        raw = await self._runtime._rpc("mobkit/run_labels/get", {"run_id": run_id})
        if isinstance(raw, dict):
            labels = raw.get("labels", {})
            if isinstance(labels, dict):
                return {str(k): str(v) for k, v in labels.items()}
        return {}

    async def delete_run_labels(self, run_id: str) -> None:
        """Remove the label set for ``run_id``."""
        await self._runtime._rpc("mobkit/run_labels/delete", {"run_id": run_id})

    # -----------------------------------------------------------------
    # Batch
    # -----------------------------------------------------------------

    async def collect_completed(self) -> list[tuple[str, RichMemberSnapshot]]:
        """Collect all members that have reached a final state."""
        from .types import RichMemberSnapshot
        raw = await self._runtime._rpc("mobkit/collect_completed")
        results: list[tuple[str, RichMemberSnapshot]] = []
        entries = raw.get("completed", []) if isinstance(raw, dict) else raw if isinstance(raw, list) else []
        for entry in entries:
            member_id = entry.get("member_id", "")
            snapshot = RichMemberSnapshot.from_dict(entry.get("snapshot", entry))
            results.append((member_id, snapshot))
        return results

    # -----------------------------------------------------------------
    # Server-side readiness
    # -----------------------------------------------------------------

    async def wait_ready(
        self,
        *,
        timeout: float | None = None,
    ) -> dict[str, Any]:
        """Wait until all current mob members are startup-ready for orchestration.

        Relays meerkat 0.6's ``MobHandle::wait_for_ready``. Returns a dict
        ``{"ready": [{"agent_identity", "snapshot"}], "timeout": bool}``.
        ``timeout=True`` means partial readiness within the deadline; the
        ``ready`` list will be empty in that case.

        :param timeout: optional seconds to wait. Omit/``None`` waits up to a
            generous server-side default ceiling (~10 minutes); pass an explicit
            value to override. The wait returns as soon as members converge — the
            ceiling is only the wall before ``timeout=True`` is reported.
        """
        params: dict[str, Any] = {}
        if timeout is not None:
            params["timeout_ms"] = int(timeout * 1000)
        raw = await self._runtime._rpc("mobkit/wait_ready", params)
        if not isinstance(raw, dict):
            return {"ready": [], "timeout": False}
        return {
            "ready": list(raw.get("ready", [])),
            "timeout": bool(raw.get("timeout", False)),
        }

    # -----------------------------------------------------------------
    # Polling helpers (client-side)
    # -----------------------------------------------------------------

    async def wait_one(
        self,
        member_id: str,
        *,
        poll_interval: float = 1.0,
        timeout: float | None = None,
    ) -> RichMemberSnapshot:
        """Poll member_status until the member reaches a final state.

        Raises ``TimeoutError`` if *timeout* seconds elapse before completion.
        """
        import asyncio
        import time

        from .types import RichMemberSnapshot

        deadline = time.monotonic() + timeout if timeout is not None else None
        while True:
            snapshot = await self.member_status(member_id)
            if snapshot.is_final:
                return snapshot
            if deadline is not None and time.monotonic() >= deadline:
                raise TimeoutError(
                    f"member {member_id!r} did not reach final state "
                    f"within {timeout}s"
                )
            await asyncio.sleep(poll_interval)

    async def wait_all(
        self,
        member_ids: list[str],
        *,
        poll_interval: float = 1.0,
        timeout: float | None = None,
    ) -> list[RichMemberSnapshot]:
        """Wait for all listed members to reach final state.

        Polls in parallel via ``asyncio.gather``. Raises ``TimeoutError``
        if *timeout* seconds elapse before all members complete.
        """
        import asyncio

        from .types import RichMemberSnapshot

        tasks = [
            self.wait_one(mid, poll_interval=poll_interval, timeout=timeout)
            for mid in member_ids
        ]
        return list(await asyncio.gather(*tasks))

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
        if auth_config is not None:
            config_dict = _auth_config_to_dict(auth_config)
            if config_dict.get("provider") == "google":
                raise ValueError(
                    "GoogleAuthConfig cannot be used with the Python ASGI facade — "
                    "asymmetric OIDC/JWKS verification requires the Rust gateway. "
                    "Use auth=auth.jwt(...) for direct ASGI deployments, or route "
                    "through the Rust gateway for Google auth."
                )
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
                # Pre-fix this used `exc.message`, an attribute that
                # never existed on `RpcError`. The handler then fell
                # through to the generic `except Exception` arm and
                # returned -32603 with an `AttributeError` message,
                # masking the real RPC code/message.
                error_payload: dict[str, Any] = {"code": exc.code, "message": str(exc)}
                if exc.data is not None:
                    error_payload["data"] = exc.data
                resp = json.dumps({
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": error_payload,
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


def _serialize_config(config: Any, _seen: set[int] | None = None) -> Any:
    """Serialize a config object to a JSON-compatible dict.

    Calls to_dict() on dataclass configs (GoogleAuthConfig, JwtAuthConfig,
    etc.) so json.dumps won't fail with TypeError during mobkit/init.
    Non-serializable leaves (e.g. storage backend instances) are converted
    to their qualified class name so the gateway receives a meaningful
    string instead of crashing the transport.  Cycle-safe via id-set.
    """
    if config is None or isinstance(config, (bool, int, float, str)):
        return config
    obj_id = id(config)
    if _seen is None:
        _seen = set()
    if obj_id in _seen:
        return f"[circular:{type(config).__qualname__}]"
    _seen.add(obj_id)
    if hasattr(config, "to_dict"):
        return config.to_dict()
    if isinstance(config, dict):
        return {k: _serialize_config(v, _seen) for k, v in config.items()}
    if isinstance(config, (list, tuple)):
        return [_serialize_config(v, _seen) for v in config]
    # Non-serializable object — use qualified class name so the gateway
    # gets a meaningful identifier instead of a TypeError.
    return f"{type(config).__module__}.{type(config).__qualname__}"


def _auth_config_to_dict(auth_config: Any) -> dict[str, Any]:
    """Normalize an auth config to a plain dict.

    Accepts JwtAuthConfig/GoogleAuthConfig (with to_dict()), plain dicts,
    or anything else (returns empty dict for fail-closed behavior).
    """
    if hasattr(auth_config, "to_dict"):
        return auth_config.to_dict()
    if isinstance(auth_config, dict):
        return auth_config
    return {}


def _validate_bearer_token(token: str, auth_config: Any) -> bool:
    """Validate a bearer token against the auth config.

    For JwtAuthConfig / {"provider": "jwt"} (shared-secret HMAC): full
    signature verification plus iss, aud, exp, and nbf claim checks
    with leeway.

    Google and unknown providers are rejected (fail closed). Google auth
    is blocked at AsgiApp construction time, but this is a defense-in-depth
    fallback.
    """
    config_dict = _auth_config_to_dict(auth_config)
    provider = config_dict.get("provider", "")

    if provider != "jwt":
        return False

    parts = token.split(".")
    if len(parts) != 3:
        return False

    try:
        header_b64 = parts[0] + "=" * (-len(parts[0]) % 4)
        header = json.loads(base64.urlsafe_b64decode(header_b64))
    except Exception:
        return False

    # Header must be a dict with alg field.
    if not isinstance(header, dict):
        return False

    secret = config_dict.get("shared_secret", "")
    if not secret:
        return False
    if header.get("alg") != "HS256":
        return False

    # Verify HMAC-SHA256 signature
    signing_input = f"{parts[0]}.{parts[1]}".encode("utf-8")
    expected_sig = base64.urlsafe_b64encode(
        hmac.new(secret.encode("utf-8"), signing_input, hashlib.sha256).digest()
    ).rstrip(b"=").decode("utf-8")
    if not hmac.compare_digest(expected_sig, parts[2]):
        return False

    # Decode and validate claims — must be a JSON object.
    try:
        payload_b64 = parts[1] + "=" * (-len(parts[1]) % 4)
        claims = json.loads(base64.urlsafe_b64decode(payload_b64))
    except Exception:
        return False

    if not isinstance(claims, dict):
        return False

    if config_dict.get("issuer") and claims.get("iss") != config_dict["issuer"]:
        return False
    expected_aud = config_dict.get("audience")
    if expected_aud:
        token_aud = claims.get("aud")
        # JWT aud may be a string or an array of strings (RFC 7519 §4.1.3).
        if isinstance(token_aud, list):
            if expected_aud not in token_aud:
                return False
        elif token_aud != expected_aud:
            return False

    # Enforce expiry with leeway — exp/nbf must be numeric.
    leeway = config_dict.get("leeway_seconds", 60)
    now = time.time()
    exp = claims.get("exp")
    if exp is not None:
        if not isinstance(exp, (int, float)):
            return False
        if now > exp + leeway:
            return False
    nbf = claims.get("nbf")
    if nbf is not None:
        if not isinstance(nbf, (int, float)):
            return False
        if now < nbf - leeway:
            return False

    return True


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
