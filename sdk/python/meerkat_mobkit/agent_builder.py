"""SessionAgentBuilder protocol — imperative mutation pattern matching HomeCore."""
from __future__ import annotations

import asyncio
import inspect
import logging
from typing import Any, Callable, Protocol, runtime_checkable

from .models import SessionBuildOptions
from .types import ErrorEvent

_log = logging.getLogger("meerkat_mobkit")


@runtime_checkable
class SessionAgentBuilder(Protocol):
    """Protocol for building agents during session creation.

    Uses the imperative mutation pattern: build_agent receives a mutable
    SessionBuildOptions and modifies it (sets profile_name, calls add_tools
    or register_tool).

    Example:
        class MyAgentBuilder(SessionAgentBuilder):
            async def build_agent(self, opts: SessionBuildOptions) -> None:
                opts.profile_name = "assistant"
                opts.register_tool("search", my_search_handler)
                opts.register_tool("calc", my_calc_handler)
    """

    async def build_agent(self, options: SessionBuildOptions) -> None:
        """Build an agent by mutating the given options.

        Args:
            options: Mutable SessionBuildOptions. Set profile_name,
                    additional_instructions, and call register_tool() or add_tools().
        """
        ...


class CallbackDispatcher:
    """Routes incoming JSON-RPC callback requests from the Rust runtime
    to the registered SessionAgentBuilder and tool handlers.

    Tool handlers are scoped by a build-level scope_id to prevent
    cross-session handler bleed in concurrent sessions.
    """

    def __init__(self) -> None:
        self._builder: SessionAgentBuilder | None = None
        self._error_callback: Callable | None = None
        # Keyed by (scope_id, tool_name) to isolate concurrent sessions
        self._tool_handlers: dict[tuple[str, str], Any] = {}
        # Track scope_ids so we can clean up handlers when a scope is released
        self._scope_tools: dict[str, list[str]] = {}
        # Identity-first providers (REQ-45)
        self._continuity_store: Any | None = None
        self._lease_provider: Any | None = None
        self._roster_provider: Any | None = None
        self._topology_provider: Any | None = None
        self._agent_customizer: Any | None = None

    def register_builder(self, builder: SessionAgentBuilder) -> None:
        self._builder = builder

    def register_error_callback(self, callback: Callable) -> None:
        self._error_callback = callback

    def register_continuity_store(self, provider: Any) -> None:
        self._continuity_store = provider

    def register_lease_provider(self, provider: Any) -> None:
        self._lease_provider = provider

    def register_roster_provider(self, provider: Any) -> None:
        self._roster_provider = provider

    def register_topology_provider(self, provider: Any) -> None:
        self._topology_provider = provider

    def register_agent_customizer(self, provider: Any) -> None:
        self._agent_customizer = provider

    def release_scope(self, scope_id: str) -> None:
        """Remove all tool handlers for a scope. Call when a session ends."""
        for tool_name in self._scope_tools.pop(scope_id, []):
            self._tool_handlers.pop((scope_id, tool_name), None)

    async def handle_callback(self, method: str, params: dict[str, Any]) -> Any:
        if method == "mobkit/on_error":
            if self._error_callback is not None:
                event = ErrorEvent.from_dict(params)
                try:
                    result = self._error_callback(event)
                    if inspect.isawaitable(result):
                        await result
                except Exception as exc:
                    _log.warning("error callback failed: %s", exc)
            return None

        if method == "callback/after_create":
            if self._builder is not None and hasattr(self._builder, "after_create"):
                from .models import SessionCreatedContext

                session_id = params.get("session_id", "")
                context = SessionCreatedContext.from_dict(params)
                try:
                    result = self._builder.after_create(session_id, context)
                    if asyncio.iscoroutine(result):
                        await result
                except Exception as exc:
                    _log.warning("after_create callback failed: %s", exc)
            return None

        if method == "callback/build_agent":
            if self._builder is None:
                raise ValueError("no SessionAgentBuilder registered")
            raw_options = dict(params.get("options", {}))
            scope_id = raw_options.pop("scope_id", None)
            if not scope_id:
                raise ValueError("callback/build_agent requires scope_id in options")
            # Filter to only fields accepted by SessionBuildOptions — Rust
            # sends extra context (model, prompt) that is informational only.
            import dataclasses as _dc
            _known = {f.name for f in _dc.fields(SessionBuildOptions)}
            filtered = {k: v for k, v in raw_options.items() if k in _known}
            opts = SessionBuildOptions(**filtered)
            await self._builder.build_agent(opts)
            for t in opts.tools:
                if not isinstance(t, str):
                    raise TypeError(
                        f"build_agent produced non-string tool {type(t).__name__}: {t!r}"
                    )
            # Capture tool handlers scoped to this build's scope_id
            tool_names = []
            for name, handler in opts.tool_handlers.items():
                self._tool_handlers[(scope_id, name)] = handler
                tool_names.append(name)
            self._scope_tools[scope_id] = tool_names
            return opts.to_dict()

        if method == "callback/call_tool":
            scope_id = params.get("scope_id")
            if not scope_id:
                raise ValueError("callback/call_tool requires scope_id")
            tool_name = params.get("tool", "")
            arguments = params.get("arguments", {})
            handler = self._tool_handlers.get((scope_id, tool_name))
            if handler is None:
                raise ValueError(
                    f"no handler registered for tool: {tool_name} (scope: {scope_id})"
                )
            result = handler(arguments)
            if asyncio.iscoroutine(result):
                result = await result
            return {"content": result}

        # ----- Identity-first provider routing (REQ-45) -----
        if method.startswith("callback/continuity_store/"):
            return await self._handle_continuity_store(method, params)
        if method.startswith("callback/lease_provider/"):
            return await self._handle_lease_provider(method, params)
        if method.startswith("callback/roster_provider/"):
            return await self._handle_roster_provider(method, params)
        if method.startswith("callback/topology_provider/"):
            return await self._handle_topology_provider(method, params)
        if method.startswith("callback/agent_customizer/"):
            return await self._handle_agent_customizer(method, params)

        raise ValueError(f"unknown callback method: {method}")

    # --- Provider dispatch helpers ---

    async def _handle_continuity_store(self, method: str, params: dict[str, Any]) -> Any:
        from .identity_first_providers import (
            ContinuityRecord,
            SessionSnapshot,
        )
        store = self._continuity_store
        if store is None:
            raise ValueError("no continuity store provider registered")

        op = method.rsplit("/", 1)[-1]

        if op == "resolve_many":
            result = await store.resolve_many(params["identities"])
            return {k: v.to_dict() for k, v in result.items()}

        if op == "load_session_snapshot":
            snap = await store.load_session_snapshot(params["session_id"])
            return snap.to_dict() if snap is not None else None

        if op == "save_session_snapshot":
            snapshot = SessionSnapshot.from_dict(params["snapshot"])
            await store.save_session_snapshot(
                params["identity"],
                params["session_id"],
                params["generation"],
                params["version"],
                params["fencing_token"],
                snapshot,
            )
            return None

        if op == "upsert_continuity_record":
            record = ContinuityRecord.from_dict(params["record"])
            await store.upsert_continuity_record(record, params["fencing_token"])
            return None

        raise ValueError(f"unknown continuity_store operation: {op}")

    async def _handle_lease_provider(self, method: str, params: dict[str, Any]) -> Any:
        from .identity_first_providers import LeaseGrant
        provider = self._lease_provider
        if provider is None:
            raise ValueError("no lease provider registered")

        op = method.rsplit("/", 1)[-1]

        if op == "acquire_leases":
            result = await provider.acquire_leases(
                params["identities"], params["runtime_instance"],
            )
            return {k: v.to_dict() for k, v in result.items()}

        if op == "renew_leases":
            grants = [LeaseGrant.from_dict(g) for g in params["grants"]]
            result = await provider.renew_leases(grants)
            return {k: v.to_dict() for k, v in result.items()}

        if op == "release_leases":
            grants = [LeaseGrant.from_dict(g) for g in params["grants"]]
            await provider.release_leases(grants)
            return None

        raise ValueError(f"unknown lease_provider operation: {op}")

    async def _handle_roster_provider(self, method: str, params: dict[str, Any]) -> Any:
        provider = self._roster_provider
        if provider is None:
            raise ValueError("no roster provider registered")

        op = method.rsplit("/", 1)[-1]

        if op == "roster":
            specs = await provider.roster(params.get("context", {}))
            return [s.to_dict() for s in specs]

        raise ValueError(f"unknown roster_provider operation: {op}")

    async def _handle_topology_provider(self, method: str, params: dict[str, Any]) -> Any:
        provider = self._topology_provider
        if provider is None:
            raise ValueError("no topology provider registered")

        op = method.rsplit("/", 1)[-1]

        if op == "compute_edges":
            edges = await provider.compute_edges(
                params["target_identities"],
                params.get("context", {}),
            )
            return [e.to_dict() for e in edges]

        raise ValueError(f"unknown topology_provider operation: {op}")

    async def _handle_agent_customizer(self, method: str, params: dict[str, Any]) -> Any:
        from .identity_first_models import (
            AgentBuildContext,
            AgentBuildDraft,
            DurableAgentSpec,
        )
        from .models import SessionCreatedContext

        customizer = self._agent_customizer
        if customizer is None:
            raise ValueError("no agent customizer registered")

        op = method.rsplit("/", 1)[-1]

        if op == "customize_build":
            context = AgentBuildContext.from_dict(params["context"])
            spec = DurableAgentSpec.from_dict(params["spec"])
            draft = AgentBuildDraft.from_dict(params["draft"])
            await customizer.customize_build(context, spec, draft)
            return draft.to_dict()

        if op == "after_create":
            identity = params["identity"]
            session_id = params["session_id"]
            context = SessionCreatedContext.from_dict(params.get("context", {}))
            if hasattr(customizer, "after_create"):
                result = customizer.after_create(identity, session_id, context)
                if asyncio.iscoroutine(result):
                    await result
            return None

        raise ValueError(f"unknown agent_customizer operation: {op}")
