"""Identity-first models for MobKit SDK (REQ-40, REQ-43, REQ-43a)."""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any

_VALID_ORIGINS = frozenset({"connector", "scheduler", "policy", "flow", "system"})


# ---------------------------------------------------------------------------
# Content blocks (REQ-43)
# ---------------------------------------------------------------------------

class ContentBlock:
    """Base for typed content blocks. Use from_dict() to deserialize."""

    @staticmethod
    def from_dict(data: dict[str, Any]) -> ContentBlock:
        t = data.get("type")
        if t == "text":
            return TextBlock(text=data["text"])
        elif t == "image":
            source = data.get("source", "inline")
            if source == "blob":
                return ImageBlock(
                    media_type=data["media_type"],
                    source="blob",
                    blob_id=data["blob_id"],
                )
            if source != "inline":
                raise ValueError("image source must be 'inline' or 'blob'")
            return ImageBlock(
                media_type=data["media_type"],
                data=data["data"],
                source="inline",
            )
        raise ValueError(f"unknown content block type: {t!r}")

    def to_dict(self) -> dict[str, Any]:
        raise NotImplementedError


@dataclass
class TextBlock(ContentBlock):
    text: str

    def to_dict(self) -> dict[str, Any]:
        return {"type": "text", "text": self.text}


@dataclass
class ImageBlock(ContentBlock):
    media_type: str
    data: str = ""
    source: str = "inline"
    blob_id: str | None = None

    def __post_init__(self) -> None:
        if self.source not in {"inline", "blob"}:
            raise ValueError("image source must be 'inline' or 'blob'")
        if self.source == "blob" and not self.blob_id:
            raise ValueError("blob image blocks require blob_id")

    def to_dict(self) -> dict[str, Any]:
        if self.source == "blob":
            return {
                "type": "image",
                "media_type": self.media_type,
                "source": "blob",
                "blob_id": self.blob_id,
            }
        return {
            "type": "image",
            "media_type": self.media_type,
            "source": "inline",
            "data": self.data,
        }


# ---------------------------------------------------------------------------
# DurableAgentSpec (REQ-40)
# ---------------------------------------------------------------------------

@dataclass
class DurableAgentSpec:
    """Identity-first agent specification matching Rust DurableAgentSpec."""

    identity: str
    profile: str
    addressability: str = "addressable"
    display_name: str | None = None
    labels: dict[str, str] = field(default_factory=dict)
    context: Any | None = None
    additional_instructions: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {
            "identity": self.identity,
            "profile": self.profile,
            "addressability": self.addressability,
        }
        if self.display_name is not None:
            result["display_name"] = self.display_name
        result["labels"] = dict(self.labels)
        if self.context is not None:
            result["context"] = self.context
        result["additional_instructions"] = list(self.additional_instructions)
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> DurableAgentSpec:
        return cls(
            identity=data["identity"],
            profile=data["profile"],
            addressability=data.get("addressability", "addressable"),
            display_name=data.get("display_name"),
            labels=dict(data.get("labels") or {}),
            context=data.get("context"),
            additional_instructions=list(data.get("additional_instructions") or []),
        )


# ---------------------------------------------------------------------------
# DispatchInput (REQ-43)
# ---------------------------------------------------------------------------

@dataclass
class DispatchInput:
    """Typed dispatch input matching Rust DispatchInput."""

    content: str | list[ContentBlock]
    origin: str
    correlation_id: str | None = None
    idempotency_key: str | None = None

    def __post_init__(self) -> None:
        if self.origin not in _VALID_ORIGINS:
            raise ValueError(
                f"origin must be one of {sorted(_VALID_ORIGINS)}, got {self.origin!r}"
            )

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {"origin": self.origin}
        if isinstance(self.content, str):
            result["content"] = self.content
        else:
            result["content"] = [b.to_dict() for b in self.content]
        if self.correlation_id is not None:
            result["correlation_id"] = self.correlation_id
        if self.idempotency_key is not None:
            result["idempotency_key"] = self.idempotency_key
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> DispatchInput:
        raw_content = data["content"]
        if isinstance(raw_content, str):
            content: str | list[ContentBlock] = raw_content
        else:
            content = [ContentBlock.from_dict(b) for b in raw_content]
        return cls(
            content=content,
            origin=data["origin"],
            correlation_id=data.get("correlation_id"),
            idempotency_key=data.get("idempotency_key"),
        )

    @classmethod
    def system(cls, content: str) -> DispatchInput:
        """System-origin dispatch from plain text."""
        return cls(content=content, origin="system")

    @classmethod
    def connector(cls, content: str, *, correlation_id: str | None = None) -> DispatchInput:
        """Connector-origin dispatch."""
        return cls(content=content, origin="connector", correlation_id=correlation_id)

    @classmethod
    def scheduler(cls, content: str) -> DispatchInput:
        """Scheduler-origin dispatch."""
        return cls(content=content, origin="scheduler")


# ---------------------------------------------------------------------------
# ManagedPeerEdge (REQ-43a)
# ---------------------------------------------------------------------------

@dataclass
class ManagedPeerEdge:
    """A managed topology edge between two agent identities."""

    a: str
    b: str

    def to_dict(self) -> dict[str, Any]:
        return {"a": self.a, "b": self.b}

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ManagedPeerEdge:
        return cls(a=data["a"], b=data["b"])


# ---------------------------------------------------------------------------
# ExternalToolDef, AgentBuildContext, AgentBuildDraft (REQ-43a)
# ---------------------------------------------------------------------------

@dataclass
class ExternalToolDef:
    """Tool definition for the customizer boundary."""

    name: str
    description: str
    input_schema: dict[str, Any]

    def to_dict(self) -> dict[str, Any]:
        return {
            "name": self.name,
            "description": self.description,
            "input_schema": self.input_schema,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ExternalToolDef:
        return cls(
            name=data["name"],
            description=data["description"],
            input_schema=data["input_schema"],
        )


@dataclass(frozen=True)
class AgentBuildContext:
    """Read-only context provided to AgentCustomizer at build time."""

    identity: str
    active_peers: list[str]
    managed_edges: list[ManagedPeerEdge]

    def to_dict(self) -> dict[str, Any]:
        return {
            "identity": self.identity,
            "active_peers": list(self.active_peers),
            "managed_edges": [e.to_dict() for e in self.managed_edges],
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> AgentBuildContext:
        return cls(
            identity=data["identity"],
            active_peers=list(data.get("active_peers") or []),
            managed_edges=[
                ManagedPeerEdge.from_dict(e)
                for e in (data.get("managed_edges") or [])
            ],
        )


@dataclass
class AgentBuildDraft:
    """Mutable draft that AgentCustomizer modifies."""

    model: str | None = None
    system_prompt: str | None = None
    additional_instructions: list[str] = field(default_factory=list)
    labels: dict[str, str] = field(default_factory=dict)
    app_context: Any | None = None
    external_tools: list[ExternalToolDef] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {}
        if self.model is not None:
            result["model"] = self.model
        if self.system_prompt is not None:
            result["system_prompt"] = self.system_prompt
        result["additional_instructions"] = list(self.additional_instructions)
        result["labels"] = dict(self.labels)
        if self.app_context is not None:
            result["app_context"] = self.app_context
        result["external_tools"] = [t.to_dict() for t in self.external_tools]
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> AgentBuildDraft:
        return cls(
            model=data.get("model"),
            system_prompt=data.get("system_prompt"),
            additional_instructions=list(data.get("additional_instructions") or []),
            labels=dict(data.get("labels") or {}),
            app_context=data.get("app_context"),
            external_tools=[
                ExternalToolDef.from_dict(t)
                for t in (data.get("external_tools") or [])
            ],
        )


# ---------------------------------------------------------------------------
# IdentityStatus + supporting types (REQ-43b)
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class DurabilityPolicy:
    """Durability policy for continuity store."""

    kind: str
    max_loss_window_ms: int | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {"kind": self.kind}
        if self.max_loss_window_ms is not None:
            result["max_loss_window_ms"] = self.max_loss_window_ms
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> DurabilityPolicy:
        return cls(
            kind=data["kind"],
            max_loss_window_ms=data.get("max_loss_window_ms"),
        )


@dataclass(frozen=True)
class LeaseInfo:
    """Lease state for an identity."""

    fencing_token: int
    ttl_remaining_ms: int
    healthy: bool

    def to_dict(self) -> dict[str, Any]:
        return {
            "fencing_token": self.fencing_token,
            "ttl_remaining_ms": self.ttl_remaining_ms,
            "healthy": self.healthy,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> LeaseInfo:
        return cls(
            fencing_token=data["fencing_token"],
            ttl_remaining_ms=data["ttl_remaining_ms"],
            healthy=data["healthy"],
        )


@dataclass(frozen=True)
class ContinuityHealth:
    """Continuity store health for an identity."""

    store_reachable: bool
    durability_policy: DurabilityPolicy
    last_checkpoint_version: int | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {
            "store_reachable": self.store_reachable,
            "durability_policy": self.durability_policy.to_dict(),
        }
        if self.last_checkpoint_version is not None:
            result["last_checkpoint_version"] = self.last_checkpoint_version
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ContinuityHealth:
        return cls(
            store_reachable=data["store_reachable"],
            durability_policy=DurabilityPolicy.from_dict(data["durability_policy"]),
            last_checkpoint_version=data.get("last_checkpoint_version"),
        )


@dataclass(frozen=True)
class IdentityStatus:
    """Full identity status (TYPE-18)."""

    identity: str
    state: str
    agent_runtime_id: str | None = None
    session_id: str | None = None
    profile: str | None = None
    addressability: str = "addressable"
    display_name: str | None = None
    labels: dict[str, str] = field(default_factory=dict)
    generation: int | None = None
    checkpoint_version: int | None = None
    lease: LeaseInfo | None = None
    continuity_health: ContinuityHealth | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {
            "identity": self.identity,
            "state": self.state,
            "addressability": self.addressability,
            "labels": dict(self.labels),
        }
        if self.agent_runtime_id is not None:
            result["agent_runtime_id"] = self.agent_runtime_id
        if self.session_id is not None:
            result["session_id"] = self.session_id
        if self.profile is not None:
            result["profile"] = self.profile
        if self.display_name is not None:
            result["display_name"] = self.display_name
        if self.generation is not None:
            result["generation"] = self.generation
        if self.checkpoint_version is not None:
            result["checkpoint_version"] = self.checkpoint_version
        if self.lease is not None:
            result["lease"] = self.lease.to_dict()
        if self.continuity_health is not None:
            result["continuity_health"] = self.continuity_health.to_dict()
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> IdentityStatus:
        lease_raw = data.get("lease")
        ch_raw = data.get("continuity_health")
        return cls(
            identity=data["identity"],
            state=data["state"],
            agent_runtime_id=data.get("agent_runtime_id"),
            session_id=data.get("session_id"),
            profile=data.get("profile"),
            addressability=data.get("addressability", "addressable"),
            display_name=data.get("display_name"),
            labels=dict(data.get("labels") or {}),
            generation=data.get("generation"),
            checkpoint_version=data.get("checkpoint_version"),
            lease=LeaseInfo.from_dict(lease_raw) if lease_raw else None,
            continuity_health=ContinuityHealth.from_dict(ch_raw) if ch_raw else None,
        )


# ---------------------------------------------------------------------------
# Typed return models
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class SendResult:
    """Typed result from identity-first send()."""

    fencing_token: int

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> SendResult:
        return cls(fencing_token=int(data.get("fencing_token", 0)))


@dataclass(frozen=True)
class DispatchResult:
    """Typed result from identity-first dispatch()."""

    fencing_token: int
    durable: bool

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> DispatchResult:
        return cls(
            fencing_token=int(data.get("fencing_token", 0)),
            durable=bool(data.get("durable", False)),
        )


@dataclass(frozen=True)
class IdentityInspection:
    """Rich inspection of an identity's current execution state."""

    identity: str
    output_preview: str | None = None
    is_final: bool = False
    peer_reachable_count: int = 0

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> IdentityInspection:
        return cls(
            identity=data["identity"],
            output_preview=data.get("output_preview"),
            is_final=bool(data.get("is_final", False)),
            peer_reachable_count=int(data.get("peer_reachable_count", 0)),
        )
