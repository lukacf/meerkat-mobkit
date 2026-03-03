"""SessionAgentBuilder protocol — imperative mutation pattern matching HomeCore."""
from __future__ import annotations

from typing import Any, Protocol, runtime_checkable

from .models import SessionBuildOptions


@runtime_checkable
class SessionAgentBuilder(Protocol):
    """Protocol for building agents during session creation.

    Uses the imperative mutation pattern: build_agent receives a mutable
    SessionBuildOptions and modifies it (sets profile_name, calls add_tools).

    Example:
        class MyAgentBuilder(SessionAgentBuilder):
            async def build_agent(self, opts: SessionBuildOptions) -> None:
                opts.profile_name = "assistant"
                opts.additional_instructions = "Be helpful."
                opts.add_tools([search_tool, calc_tool])
    """

    async def build_agent(self, options: SessionBuildOptions) -> None:
        """Build an agent by mutating the given options.

        Args:
            options: Mutable SessionBuildOptions. Set profile_name,
                    additional_instructions, and call add_tools().
        """
        ...


class CallbackDispatcher:
    """Routes incoming JSON-RPC callback requests from the Rust runtime
    to the registered SessionAgentBuilder."""

    def __init__(self) -> None:
        self._builder: SessionAgentBuilder | None = None

    def register_builder(self, builder: SessionAgentBuilder) -> None:
        self._builder = builder

    async def handle_callback(self, method: str, params: dict[str, Any]) -> Any:
        if method == "callback/build_agent":
            if self._builder is None:
                raise ValueError("no SessionAgentBuilder registered")
            opts = SessionBuildOptions(**(params.get("options", {})))
            await self._builder.build_agent(opts)
            return opts.to_dict()
        raise ValueError(f"unknown callback method: {method}")
