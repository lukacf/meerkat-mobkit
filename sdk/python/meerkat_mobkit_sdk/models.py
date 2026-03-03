"""Typed data models for MobKit SDK."""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class DiscoverySpec:
    """Specification for discovering an agent to spawn.

    Maps to Rust SpawnMemberSpec fields via the MobKit discovery pipeline.
    """

    profile: str
    meerkat_id: str
    labels: dict[str, str] = field(default_factory=dict)
    context: Any | None = None
    additional_instructions: str | None = None
    resume_session_id: str | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {
            "profile": self.profile,
            "meerkat_id": self.meerkat_id,
        }
        if self.labels:
            result["labels"] = dict(self.labels)
        if self.context is not None:
            result["context"] = self.context
        if self.additional_instructions is not None:
            result["additional_instructions"] = self.additional_instructions
        if self.resume_session_id is not None:
            result["resume_session_id"] = self.resume_session_id
        return result


@dataclass(frozen=True)
class PreSpawnData:
    """Data passed to modules before agent spawning.

    Maps to Rust PreSpawnData.
    """

    module_id: str
    env: dict[str, str] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        return {
            "module_id": self.module_id,
            "env": list(self.env.items()),
        }


@dataclass(frozen=True)
class SessionQuery:
    """Query parameters for session lookup.

    Used to filter sessions by labels or other criteria.
    """

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
class SessionBuildOptions:
    """Options passed to SessionAgentBuilder.build_agent().

    Carries application context and instructions for agent construction.
    """

    app_context: Any | None = None
    additional_instructions: str | None = None
    session_id: str | None = None
    labels: dict[str, str] = field(default_factory=dict)

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {}
        if self.app_context is not None:
            result["app_context"] = self.app_context
        if self.additional_instructions is not None:
            result["additional_instructions"] = self.additional_instructions
        if self.session_id is not None:
            result["session_id"] = self.session_id
        if self.labels:
            result["labels"] = dict(self.labels)
        return result
