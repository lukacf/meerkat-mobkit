"""SessionAgentBuilder protocol for MobKit agent construction.

The Rust runtime calls into Python via bidirectional JSON-RPC when it
needs to build an agent.  The Python process implements the builder
protocol and returns the agent configuration.
"""

from __future__ import annotations

from typing import Any, Protocol, runtime_checkable


@runtime_checkable
class SessionAgentBuilder(Protocol):
    """Protocol for building agents during session creation.

    Implement this protocol to customize how agents are constructed.
    The MobKit runtime calls ``build_agent()`` when a new agent session
    needs to be created (e.g. during discovery or on-demand spawn).

    Example::

        class MyAgentBuilder(SessionAgentBuilder):
            async def build_agent(self, options: SessionBuildOptions) -> dict[str, Any]:
                return {
                    "system_prompt": f"You are {options.app_context['role']}",
                    "tools": ["search", "calculate"],
                    "model": "claude-sonnet-4-6",
                }
    """

    async def build_agent(self, options: Any) -> dict[str, Any]:
        """Build an agent configuration from the given options.

        Args:
            options: SessionBuildOptions containing app_context,
                    additional_instructions, session_id, and labels.

        Returns:
            Agent configuration dict with keys like system_prompt,
            tools, model, etc.
        """
        ...


class CallbackDispatcher:
    """Handles incoming JSON-RPC callback requests from the Rust runtime.

    When the Rust runtime needs the Python process to build an agent,
    it sends a JSON-RPC request over the persistent transport.  This
    dispatcher routes those requests to the registered builder.
    """

    def __init__(self) -> None:
        self._builder: SessionAgentBuilder | None = None

    def register_builder(self, builder: SessionAgentBuilder) -> None:
        """Register a SessionAgentBuilder to handle build_agent callbacks."""
        self._builder = builder

    async def handle_callback(self, method: str, params: dict[str, Any]) -> Any:
        """Handle an incoming callback request from the Rust runtime.

        Args:
            method: The JSON-RPC method name (e.g. ``"callback/build_agent"``).
            params: The method parameters.

        Returns:
            The result to send back to the Rust runtime.

        Raises:
            ValueError: If the method is unknown or no builder is registered.
        """
        if method == "callback/build_agent":
            if self._builder is None:
                raise ValueError("no SessionAgentBuilder registered")
            return await self._builder.build_agent(params.get("options", {}))

        raise ValueError(f"unknown callback method: {method}")

    def handle_callback_sync(self, method: str, params: dict[str, Any]) -> Any:
        """Synchronous wrapper for :meth:`handle_callback`."""
        import asyncio
        import concurrent.futures

        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            loop = None

        if loop is not None and loop.is_running():
            with concurrent.futures.ThreadPoolExecutor(max_workers=1) as pool:
                future = pool.submit(
                    asyncio.run, self.handle_callback(method, params)
                )
                return future.result()
        else:
            return asyncio.run(self.handle_callback(method, params))
