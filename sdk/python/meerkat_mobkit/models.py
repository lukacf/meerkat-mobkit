"""Typed data models for MobKit SDK — matches HomeCore import surface."""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class DiscoverySpec:
    """Agent discovery specification.

    Maps to Rust SpawnMemberSpec fields via the MobKit discovery pipeline.
    """

    profile: str
    meerkat_id: str
    labels: dict[str, str] = field(default_factory=dict)
    app_context: Any | None = None
    additional_instructions: list[str] = field(default_factory=list)
    resume_session_id: str | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {
            "profile": self.profile,
            "meerkat_id": self.meerkat_id,
        }
        if self.labels:
            result["labels"] = dict(self.labels)
        if self.app_context is not None:
            result["app_context"] = self.app_context
        if self.additional_instructions:
            result["additional_instructions"] = list(self.additional_instructions)
        if self.resume_session_id is not None:
            result["resume_session_id"] = self.resume_session_id
        return result


@dataclass
class PreSpawnData:
    """Pre-spawn data for session resume and cache warming.

    The resume_map maps meerkat_id -> session_id for agents that should
    resume existing sessions instead of creating new ones.
    """

    resume_map: dict[str, str] = field(default_factory=dict)
    module_id: str | None = None
    env: dict[str, str] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {}
        if self.resume_map:
            result["resume_map"] = dict(self.resume_map)
        if self.module_id is not None:
            result["module_id"] = self.module_id
        if self.env:
            result["env"] = list(self.env.items())
        return result


@dataclass
class SessionQuery:
    """Query parameters for session lookup."""

    agent_type: str | None = None
    owner_id: str | None = None
    labels: dict[str, str] = field(default_factory=dict)
    include_deleted: bool = False
    limit: int = 100

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {}
        if self.agent_type is not None:
            result["agent_type"] = self.agent_type
        if self.owner_id is not None:
            result["owner_id"] = self.owner_id
        if self.labels:
            result["labels"] = dict(self.labels)
        result["include_deleted"] = self.include_deleted
        result["limit"] = self.limit
        return result


@dataclass
class SessionBuildOptions:
    """Options passed to SessionAgentBuilder.build_agent().

    Mutable — the builder mutates fields during agent construction.
    """

    app_context: Any | None = None
    additional_instructions: list[str] = field(default_factory=list)
    session_id: str | None = None
    labels: dict[str, str] = field(default_factory=dict)
    profile_name: str | None = None
    _tools: list[str] = field(default_factory=list, repr=False)
    _tool_handlers: dict[str, Any] = field(default_factory=dict, repr=False)

    def add_tools(self, tools: list[str]) -> None:
        """Declare tool names the agent can use."""
        for t in tools:
            if not isinstance(t, str):
                raise TypeError(f"tools must be strings, got {type(t).__name__}: {t!r}")
        self._tools.extend(tools)

    def register_tool(self, name: str, handler: Any) -> None:
        """Register a callable tool with the agent.

        The handler is called when the agent invokes this tool. It receives
        a dict of arguments and should return a JSON-serializable result.

        Args:
            name: Tool name (string).
            handler: Async or sync callable ``(args: dict) -> Any``.
        """
        if not isinstance(name, str):
            raise TypeError(f"tool name must be a string, got {type(name).__name__}: {name!r}")
        self._tools.append(name)
        self._tool_handlers[name] = handler

    @property
    def tools(self) -> list[str]:
        return list(self._tools)

    @property
    def tool_handlers(self) -> dict[str, Any]:
        return dict(self._tool_handlers)

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {}
        if self.app_context is not None:
            result["app_context"] = self.app_context
        if self.additional_instructions:
            result["additional_instructions"] = list(self.additional_instructions)
        if self.session_id is not None:
            result["session_id"] = self.session_id
        if self.labels:
            result["labels"] = dict(self.labels)
        if self.profile_name is not None:
            result["profile_name"] = self.profile_name
        if self._tools:
            result["tools"] = self._tools
        return result
