"""SessionAgentBuilder protocol — imperative mutation pattern matching HomeCore."""
from __future__ import annotations

import asyncio
import inspect
from typing import Any, Protocol, runtime_checkable

from .models import SessionBuildOptions


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
        # Keyed by (scope_id, tool_name) to isolate concurrent sessions
        self._tool_handlers: dict[tuple[str, str], Any] = {}

    def register_builder(self, builder: SessionAgentBuilder) -> None:
        self._builder = builder

    async def handle_callback(self, method: str, params: dict[str, Any]) -> Any:
        if method == "callback/build_agent":
            if self._builder is None:
                raise ValueError("no SessionAgentBuilder registered")
            raw_options = dict(params.get("options", {}))
            scope_id = raw_options.pop("scope_id", "")
            opts = SessionBuildOptions(**raw_options)
            await self._builder.build_agent(opts)
            for t in opts.tools:
                if not isinstance(t, str):
                    raise TypeError(
                        f"build_agent produced non-string tool {type(t).__name__}: {t!r}"
                    )
            # Capture tool handlers scoped to this build's scope_id
            for name, handler in opts.tool_handlers.items():
                self._tool_handlers[(scope_id, name)] = handler
            return opts.to_dict()

        if method == "callback/call_tool":
            scope_id = params.get("scope_id", "")
            tool_name = params.get("tool", "")
            arguments = params.get("arguments", {})
            handler = self._tool_handlers.get((scope_id, tool_name))
            if handler is None:
                raise ValueError(
                    f"no handler registered for tool: {tool_name} (scope: {scope_id})"
                )
            result = handler(arguments)
            # Await if the handler returned a coroutine (covers async functions,
            # wrapped/decorated async callables, and partial'd coroutines)
            if asyncio.iscoroutine(result):
                result = await result
            return {"content": result}

        raise ValueError(f"unknown callback method: {method}")
