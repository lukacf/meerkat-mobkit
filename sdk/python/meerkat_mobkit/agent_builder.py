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
    to the registered SessionAgentBuilder and tool handlers."""

    def __init__(self) -> None:
        self._builder: SessionAgentBuilder | None = None
        self._tool_handlers: dict[str, Any] = {}

    def register_builder(self, builder: SessionAgentBuilder) -> None:
        self._builder = builder

    async def handle_callback(self, method: str, params: dict[str, Any]) -> Any:
        if method == "callback/build_agent":
            if self._builder is None:
                raise ValueError("no SessionAgentBuilder registered")
            opts = SessionBuildOptions(**(params.get("options", {})))
            await self._builder.build_agent(opts)
            for t in opts.tools:
                if not isinstance(t, str):
                    raise TypeError(
                        f"build_agent produced non-string tool {type(t).__name__}: {t!r}"
                    )
            # Capture tool handlers for callback/call_tool dispatch
            self._tool_handlers.update(opts.tool_handlers)
            return opts.to_dict()

        if method == "callback/call_tool":
            tool_name = params.get("tool", "")
            arguments = params.get("arguments", {})
            handler = self._tool_handlers.get(tool_name)
            if handler is None:
                raise ValueError(f"no handler registered for tool: {tool_name}")
            if inspect.iscoroutinefunction(handler):
                result = await handler(arguments)
            else:
                result = handler(arguments)
            return {"content": result}

        raise ValueError(f"unknown callback method: {method}")
