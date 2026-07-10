"""Typed data models for MobKit SDK — matches HomeCore import surface."""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass
class DiscoverySpec:
    """Agent discovery specification.

    Maps to Rust SpawnMemberSpec fields via the MobKit discovery pipeline.
    """

    role: str
    agent_identity: str
    labels: dict[str, str] = field(default_factory=dict)
    app_context: Any | None = None
    additional_instructions: list[str] = field(default_factory=list)
    resume_session_id: str | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {
            "role": self.role,
            "agent_identity": self.agent_identity,
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

    The resume_map maps agent_identity -> session_id for agents that should
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


@dataclass(frozen=True)
class SessionCreatedContext:
    """Context delivered to SessionAgentBuilder.after_create after a session
    is successfully created."""

    model: str
    labels: dict[str, str]
    system_prompt: str | None

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> SessionCreatedContext:
        return cls(
            model=data.get("model", ""),
            labels=dict(data.get("labels") or {}),
            system_prompt=data.get("system_prompt"),
        )


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
    resume_session_id: str | None = None
    _tools: list[str] = field(default_factory=list, repr=False)
    _tool_handlers: dict[str, Any] = field(default_factory=dict, repr=False)
    # Per-tool wire metadata from register_tool(description=/input_schema=);
    # tools without an entry cross the wire as bare name strings.
    _tool_defs: dict[str, dict[str, Any]] = field(default_factory=dict, repr=False)

    def add_tools(self, tools: list[str]) -> None:
        """Declare tool names the agent can use."""
        for t in tools:
            if not isinstance(t, str):
                raise TypeError(f"tools must be strings, got {type(t).__name__}: {t!r}")
        self._tools.extend(tools)

    def register_tool(
        self,
        name: str,
        handler: Any,
        *,
        description: str = "",
        input_schema: dict[str, Any] | None = None,
    ) -> None:
        """Register a callable tool with the agent.

        The handler is called when the agent invokes this tool. It receives
        a dict of arguments and should return a JSON-serializable result.

        Args:
            name: Tool name (string).
            handler: Async or sync callable ``(args: dict) -> Any``.
            description: Human-readable tool description.
            input_schema: JSON Schema for the tool arguments. When omitted
                the gateway advertises the permissive ``{"type": "object"}``.
        """
        if not isinstance(name, str):
            raise TypeError(f"tool name must be a string, got {type(name).__name__}: {name!r}")
        if not callable(handler):
            raise TypeError(f"handler must be callable, got {type(handler).__name__}: {handler!r}")
        if input_schema is not None and not isinstance(input_schema, dict):
            raise TypeError(
                f"input_schema must be a dict, got {type(input_schema).__name__}: {input_schema!r}"
            )
        self._tools.append(name)
        self._tool_handlers[name] = handler
        if description or input_schema is not None:
            tool_def: dict[str, Any] = {"name": name}
            if description:
                tool_def["description"] = description
            if input_schema is not None:
                tool_def["input_schema"] = input_schema
            self._tool_defs[name] = tool_def

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
        if self.resume_session_id is not None:
            result["resume_session_id"] = self.resume_session_id
        if self._tools:
            # Names with registered metadata cross as {name, description?,
            # input_schema?} objects; everything else stays a bare string
            # (backward-compatible with pre-0.7.30 gateways).
            result["tools"] = [
                self._tool_defs.get(name, name) for name in self._tools
            ]
        return result
