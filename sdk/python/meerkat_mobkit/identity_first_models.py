"""Identity-first models for MobKit SDK (REQ-40, REQ-43, REQ-43a)."""
from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any, Literal

_VALID_ORIGINS = frozenset({"connector", "scheduler", "policy", "flow", "system"})
MAX_IDENTITY_BACKGROUND_WARM_CONCURRENCY = 16

IdentityBootstrapModeName = Literal[
    "eager_materialize",
    "lazy_materialize",
    "lazy_with_background_warm",
]


def _required_bool(data: dict[str, Any], field_name: str) -> bool:
    if field_name not in data:
        raise ValueError(f"identity bootstrap status requires {field_name!r}")
    value = data[field_name]
    if type(value) is not bool:
        raise TypeError(f"identity bootstrap {field_name} must be a boolean")
    return value


def _optional_bool(
    data: dict[str, Any],
    field_name: str,
    *,
    default: bool | None,
) -> bool | None:
    if field_name not in data:
        return default
    value = data[field_name]
    if type(value) is not bool:
        raise TypeError(f"identity bootstrap {field_name} must be a boolean")
    return value


def _nonnegative_int(data: dict[str, Any], field_name: str) -> int:
    value = data.get(field_name, 0)
    if type(value) is not int:
        raise TypeError(
            f"identity bootstrap count {field_name!r} must be an integer"
        )
    if value < 0:
        raise ValueError(
            f"identity bootstrap count {field_name!r} must be non-negative"
        )
    return value


@dataclass(frozen=True)
class IdentityBootstrapMode:
    """Identity materialization policy used while the gateway starts."""

    mode: IdentityBootstrapModeName
    concurrency: int | None = None

    def __post_init__(self) -> None:
        supported = {
            "eager_materialize",
            "lazy_materialize",
            "lazy_with_background_warm",
        }
        if self.mode not in supported:
            raise ValueError(f"unsupported identity bootstrap mode: {self.mode!r}")
        if self.mode == "lazy_with_background_warm":
            if (
                not isinstance(self.concurrency, int)
                or isinstance(self.concurrency, bool)
                or self.concurrency <= 0
            ):
                raise ValueError(
                    "lazy_with_background_warm requires a positive integer concurrency"
                )
            if self.concurrency > MAX_IDENTITY_BACKGROUND_WARM_CONCURRENCY:
                raise ValueError(
                    "lazy_with_background_warm concurrency must be at most "
                    f"{MAX_IDENTITY_BACKGROUND_WARM_CONCURRENCY}"
                )
        elif self.concurrency is not None:
            raise ValueError(f"{self.mode} does not accept concurrency")

    @classmethod
    def eager_materialize(cls) -> IdentityBootstrapMode:
        return cls(mode="eager_materialize")

    @classmethod
    def lazy_materialize(cls) -> IdentityBootstrapMode:
        return cls(mode="lazy_materialize")

    @classmethod
    def lazy_with_background_warm(
        cls, *, concurrency: int
    ) -> IdentityBootstrapMode:
        return cls(mode="lazy_with_background_warm", concurrency=concurrency)

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {"mode": self.mode}
        if self.concurrency is not None:
            result["concurrency"] = self.concurrency
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> IdentityBootstrapMode:
        if not isinstance(data, dict):
            raise TypeError("identity bootstrap mode must be an object")
        mode = data.get("mode")
        if not isinstance(mode, str):
            raise ValueError("identity bootstrap mode requires a string mode")
        unsupported = sorted(set(data) - {"mode", "concurrency"})
        if unsupported:
            raise ValueError(
                "identity bootstrap mode has unsupported field(s): "
                + ", ".join(unsupported)
            )
        return cls(mode=mode, concurrency=data.get("concurrency"))  # type: ignore[arg-type]


class IdentityBootstrapState(str, Enum):
    """Transient materialization state for one identity during bootstrap."""

    DORMANT = "dormant"
    WARMING = "warming"
    ACTIVE = "active"
    BROKEN = "broken"
    UNKNOWN = "unknown"

    @classmethod
    def parse(cls, raw: Any) -> IdentityBootstrapState:
        if isinstance(raw, cls):
            return raw
        if isinstance(raw, str):
            try:
                return cls(raw)
            except ValueError:
                pass
        return cls.UNKNOWN


@dataclass(frozen=True)
class IdentityBootstrapCounts:
    dormant: int = 0
    warming: int = 0
    active: int = 0
    broken: int = 0

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> IdentityBootstrapCounts:
        if not isinstance(data, dict):
            raise TypeError("identity bootstrap counts must be an object")
        return cls(
            dormant=_nonnegative_int(data, "dormant"),
            warming=_nonnegative_int(data, "warming"),
            active=_nonnegative_int(data, "active"),
            broken=_nonnegative_int(data, "broken"),
        )

    def to_dict(self) -> dict[str, int]:
        return {
            "dormant": self.dormant,
            "warming": self.warming,
            "active": self.active,
            "broken": self.broken,
        }


@dataclass(frozen=True)
class IdentityBootstrapEntry:
    identity: str
    state: IdentityBootstrapState
    error: str | None = None

    @classmethod
    def from_dict(
        cls, identity: str, data: dict[str, Any]
    ) -> IdentityBootstrapEntry:
        if not isinstance(data, dict):
            raise TypeError(f"bootstrap status for {identity!r} must be an object")
        if "state" not in data:
            raise ValueError(f"bootstrap status for {identity!r} requires state")
        state = data["state"]
        if not isinstance(state, str):
            raise TypeError(f"bootstrap state for {identity!r} must be a string")
        error = data.get("error") if "error" in data else None
        if error is not None and not isinstance(error, str):
            raise TypeError(f"bootstrap error for {identity!r} must be a string")
        return cls(
            identity=identity,
            state=IdentityBootstrapState.parse(state),
            error=error,
        )

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {"state": self.state.value}
        if self.error is not None:
            result["error"] = self.error
        return result


@dataclass(frozen=True)
class IdentityBootstrapStatus:
    """Aggregate gateway bootstrap status returned by status/wait RPCs."""

    mode: IdentityBootstrapMode
    complete: bool
    ready: bool
    counts: IdentityBootstrapCounts
    identities: dict[str, IdentityBootstrapEntry]
    error: str | None = None
    timed_out: bool = False
    target: str | None = None
    startup_ready: bool | None = None

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> IdentityBootstrapStatus:
        if not isinstance(data, dict):
            raise TypeError("identity bootstrap status must be an object")
        mode = IdentityBootstrapMode.from_dict(data.get("mode", {}))
        counts = IdentityBootstrapCounts.from_dict(data.get("counts", {}))
        identities_raw = data.get("identities", {})
        if not isinstance(identities_raw, dict):
            raise TypeError("identity bootstrap identities must be an object")
        identities = {
            str(identity): IdentityBootstrapEntry.from_dict(
                str(identity), status
            )
            for identity, status in identities_raw.items()
        }
        target = data.get("target") if "target" in data else None
        if target is not None and not isinstance(target, str):
            raise TypeError("identity bootstrap target must be a string")
        error = data.get("error") if "error" in data else None
        if error is not None and not isinstance(error, str):
            raise TypeError("identity bootstrap error must be a string")
        return cls(
            mode=mode,
            complete=_required_bool(data, "complete"),
            ready=_required_bool(data, "ready"),
            error=error,
            counts=counts,
            identities=identities,
            timed_out=_optional_bool(
                data,
                "timed_out",
                default=False,
            ),
            target=target,
            startup_ready=_optional_bool(
                data,
                "startup_ready",
                default=None,
            ),
        )

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {
            "mode": self.mode.to_dict(),
            "complete": self.complete,
            "ready": self.ready,
            "counts": self.counts.to_dict(),
            "identities": {
                identity: status.to_dict()
                for identity, status in self.identities.items()
            },
        }
        if self.error is not None:
            result["error"] = self.error
        if self.timed_out:
            result["timed_out"] = True
        if self.target is not None:
            result["target"] = self.target
        if self.startup_ready is not None:
            result["startup_ready"] = self.startup_ready
        return result


def _validate_agent_identity(identity: str) -> str:
    if (
        not identity
        or identity.strip() != identity
        or any(ch.isspace() for ch in identity)
        or "/" in identity
    ):
        raise ValueError(f"invalid agent identity: {identity}")
    return identity


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

# Meerkat's `MobRuntimeMode` wire vocabulary (serde snake_case).
_VALID_RUNTIME_MODES = frozenset({"autonomous_host", "turn_driven"})


def _content_input_to_wire(content: str | list[ContentBlock]) -> str | list[dict[str, Any]]:
    """Serialize a Rust ``ContentInput`` (untagged: text or block list)."""
    if isinstance(content, str):
        return content
    return [block.to_dict() for block in content]


def _content_input_from_wire(raw: Any) -> str | list[ContentBlock]:
    if isinstance(raw, str):
        return raw
    if isinstance(raw, list):
        return [ContentBlock.from_dict(block) for block in raw]
    raise TypeError(
        "content input must be a string or a list of content blocks, "
        f"got {type(raw).__name__}"
    )


@dataclass
class DurableAgentSpec:
    """Identity-first agent specification matching Rust DurableAgentSpec.

    ``runtime_mode_override`` pins meerkat's runtime mode for this identity
    (``"autonomous_host"`` or ``"turn_driven"``); ``None`` defers to the
    profile's ``runtime_mode`` in mob.toml, whose meerkat default is
    ``autonomous_host``. ``initial_message`` is the first turn a fresh
    ``autonomous_host`` member runs on (plain text or content blocks, Rust
    ``ContentInput``); ``None`` leaves meerkat's fallback spawn prompt in
    place. Both fields are omitted from the wire when unset, so a roster that
    never sets them is byte-identical to earlier SDKs.
    """

    identity: str
    profile: str
    addressability: str = "addressable"
    display_name: str | None = None
    labels: dict[str, str] = field(default_factory=dict)
    context: Any | None = None
    additional_instructions: list[str] = field(default_factory=list)
    # Exact canonical Meerkat host ref. Callers must never reinterpret an
    # unavailable selected host as local placement.
    placement: str | None = None
    runtime_mode_override: str | None = None
    initial_message: str | list[ContentBlock] | None = None

    def __post_init__(self) -> None:
        self.identity = _validate_agent_identity(self.identity)
        if (
            self.runtime_mode_override is not None
            and self.runtime_mode_override not in _VALID_RUNTIME_MODES
        ):
            raise ValueError(
                "runtime_mode_override must be one of "
                f"{sorted(_VALID_RUNTIME_MODES)}, got {self.runtime_mode_override!r}"
            )
        if self.initial_message is not None and not (
            isinstance(self.initial_message, str)
            or (
                isinstance(self.initial_message, list)
                and all(isinstance(b, ContentBlock) for b in self.initial_message)
            )
        ):
            raise TypeError(
                "initial_message must be a string or a list of ContentBlock, "
                f"got {type(self.initial_message).__name__}"
            )

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
        if self.placement is not None:
            result["placement"] = self.placement
        if self.runtime_mode_override is not None:
            result["runtime_mode_override"] = self.runtime_mode_override
        if self.initial_message is not None:
            result["initial_message"] = _content_input_to_wire(self.initial_message)
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> DurableAgentSpec:
        raw_initial = data.get("initial_message")
        return cls(
            identity=data["identity"],
            profile=data["profile"],
            addressability=data.get("addressability", "addressable"),
            display_name=data.get("display_name"),
            labels=dict(data.get("labels") or {}),
            context=data.get("context"),
            additional_instructions=list(data.get("additional_instructions") or []),
            placement=data.get("placement"),
            runtime_mode_override=data.get("runtime_mode_override"),
            initial_message=(
                None if raw_initial is None else _content_input_from_wire(raw_initial)
            ),
        )


@dataclass
class RoleMigrationDeclaration:
    """A host assertion that one identity's durable member role changed.

    A durable member whose role changed refuses to resume until the host names
    it here, which is deliberate: an unintended role edit must not silently
    restamp a member's durable role, comms name and binding.

    Boot-scoped. It is read once into the gateway's session bridge, never
    persisted, and gone the moment it is absent from the next boot payload.
    Meerkat re-verifies ``from_role`` against durable state, so a mistyped
    value refuses rather than restamps, and it ignores the declaration entirely
    once the roles already agree - so leaving a completed migration in place is
    inert, not a repeat restamp.
    """

    identity: str
    from_role: str

    def __post_init__(self) -> None:
        self.identity = _validate_agent_identity(self.identity)
        if not self.from_role or self.from_role.strip() != self.from_role:
            raise ValueError(f"invalid predecessor role: {self.from_role}")

    def to_dict(self) -> dict[str, Any]:
        return {"identity": self.identity, "from_role": self.from_role}

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> RoleMigrationDeclaration:
        return cls(identity=data["identity"], from_role=data["from_role"])


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

    def __post_init__(self) -> None:
        a = _validate_agent_identity(self.a)
        b = _validate_agent_identity(self.b)
        if a == b:
            raise ValueError(f"managed peer edge cannot connect an identity to itself: {a}")
        if b < a:
            a, b = b, a
        self.a = a
        self.b = b

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
    #: Per-identity provider parameter overrides, carried verbatim to the
    #: gateway. Mirrors meerkat's ``ProviderParamsOverride`` (the type behind
    #: ``AgentBuildConfig.provider_params``): provider knobs with no MobKit
    #: vocabulary of their own, such as OpenAI ``prompt_cache_key`` /
    #: ``prompt_cache_options`` / ``prompt_cache_retention`` and Anthropic
    #: ``cache_control``. Kept as a plain mapping here because the Rust
    #: boundary parses it fail-closed — an unknown or mistyped knob is
    #: rejected there rather than dropped.
    provider_params: dict[str, Any] | None = None
    _tool_handlers: dict[str, Any] = field(default_factory=dict, repr=False, compare=False)

    def register_tool(
        self,
        name: str,
        handler: Any,
        *,
        description: str = "",
        input_schema: dict[str, Any] | None = None,
    ) -> None:
        """Register a callable external tool on this build.

        Parity with :meth:`SessionBuildOptions.register_tool`, but available in
        ``AgentCustomizer.customize_build`` — which runs on BOTH fresh create and
        restore/reconcile. The handler is dispatched in-process when the agent
        invokes the tool; it receives a dict of arguments and returns a
        JSON-serializable result. Use this to reattach identity-scoped tools
        (MCP, comms, etc.) so resumed agents keep them.

        Args:
            name: Tool name (string).
            handler: Async or sync callable ``(args: dict) -> Any``.
            description: Human-readable tool description.
            input_schema: JSON Schema for the tool arguments
                (defaults to ``{"type": "object"}``).
        """
        if not isinstance(name, str):
            raise TypeError(f"tool name must be a string, got {type(name).__name__}: {name!r}")
        if not callable(handler):
            raise TypeError(f"handler must be callable, got {type(handler).__name__}: {handler!r}")
        self.external_tools.append(
            ExternalToolDef(
                name=name,
                description=description,
                input_schema=input_schema if input_schema is not None else {"type": "object"},
            )
        )
        self._tool_handlers[name] = handler

    @property
    def tool_handlers(self) -> dict[str, Any]:
        """Handlers registered via :meth:`register_tool` (in-process only)."""
        return dict(self._tool_handlers)

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
        # The gateway replaces the draft wholesale with what it gets back, so
        # a field omitted here is a field cleared on every customized build.
        if self.provider_params is not None:
            result["provider_params"] = self.provider_params
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
            provider_params=data.get("provider_params"),
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
        # `.get()` with a default to match the TS parser, which defaults `kind`
        # to the wire form `sync_write_through` rather than raising.
        return cls(
            kind=str(data.get("kind", "sync_write_through")),
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
        # `.get()` with defaults to match the TS parser's graceful degradation
        # instead of raising KeyError on a missing key from an older gateway.
        return cls(
            fencing_token=int(data.get("fencing_token", 0)),
            ttl_remaining_ms=int(data.get("ttl_remaining_ms", 0)),
            healthy=bool(data.get("healthy", False)),
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
        # `.get()` with defaults to match the TS parser's graceful degradation.
        return cls(
            store_reachable=bool(data.get("store_reachable", False)),
            durability_policy=DurabilityPolicy.from_dict(data.get("durability_policy", {})),
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
        # `.get()` with defaults to match the TS parser's graceful degradation.
        return cls(
            identity=str(data.get("identity", "")),
            state=str(data.get("state", "")),
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


class CompletionProgress(str, Enum):
    """How an observed cursor relates to a baseline captured at dispatch."""

    PENDING = "pending"
    COMPLETED = "completed"
    INCARNATION_CHANGED = "incarnation_changed"


@dataclass(frozen=True)
class CompletionCursor:
    """Comparable completion identity for one identity's stream of turns.

    Waiting for an agent's next answer must never compare output TEXT: two
    consecutive turns can legitimately produce byte-identical output, and a
    text comparison then reports "no new turn" for the whole configured wait.
    This cursor is the comparable atom instead — never derived from output
    content, a content hash, a timestamp, or a per-poll uuid.

    ``epoch`` is the identity's lease fencing token (the runtime incarnation
    this count belongs to); ``turns`` counts turns observed as completed
    within it. Turn counts are not comparable across incarnations, so classify
    with :meth:`progress_since` rather than comparing ``turns`` directly.
    """

    epoch: int = 0
    turns: int = 0

    def to_dict(self) -> dict[str, int]:
        return {"epoch": self.epoch, "turns": self.turns}

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> CompletionCursor:
        return cls(
            epoch=int(data.get("epoch", 0)),
            turns=int(data.get("turns", 0)),
        )

    def progress_since(self, baseline: CompletionCursor) -> CompletionProgress:
        """Classify this cursor against a baseline captured before delivery."""
        if self.epoch != baseline.epoch:
            return CompletionProgress.INCARNATION_CHANGED
        if self.turns > baseline.turns:
            return CompletionProgress.COMPLETED
        return CompletionProgress.PENDING


def _completion_cursor_from(data: dict[str, Any], key: str) -> CompletionCursor | None:
    """Read an optional cursor field. Absent (older gateway) stays ``None``."""
    raw = data.get(key)
    return CompletionCursor.from_dict(raw) if isinstance(raw, dict) else None


@dataclass(frozen=True)
class SendResult:
    """Typed result from identity-first send()."""

    fencing_token: int
    #: Cursor read before delivery. Wait for an inspection cursor that reports
    #: COMPLETED against this. ``None`` when the gateway predates the field.
    completion_baseline: CompletionCursor | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {"fencing_token": self.fencing_token}
        if self.completion_baseline is not None:
            result["completion_baseline"] = self.completion_baseline.to_dict()
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> SendResult:
        return cls(
            fencing_token=int(data.get("fencing_token", 0)),
            completion_baseline=_completion_cursor_from(data, "completion_baseline"),
        )


@dataclass(frozen=True)
class DispatchResult:
    """Typed result from identity-first dispatch()."""

    fencing_token: int
    durable: bool
    #: See :attr:`SendResult.completion_baseline`.
    completion_baseline: CompletionCursor | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {
            "fencing_token": self.fencing_token,
            "durable": self.durable,
        }
        if self.completion_baseline is not None:
            result["completion_baseline"] = self.completion_baseline.to_dict()
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> DispatchResult:
        return cls(
            fencing_token=int(data.get("fencing_token", 0)),
            durable=bool(data.get("durable", False)),
            completion_baseline=_completion_cursor_from(data, "completion_baseline"),
        )


@dataclass(frozen=True)
class IdentityInspection:
    """Rich inspection of an identity's current execution state."""

    identity: str
    output_preview: str | None = None
    is_final: bool = False
    peer_reachable_count: int = 0
    #: Live completion cursor. ``None`` for non-identity-first live aliases
    #: (no identity authority tracks their turns) and for gateways predating
    #: the field — in both cases the caller must not read absence as "no turns
    #: yet".
    completion_cursor: CompletionCursor | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {
            "identity": self.identity,
            "is_final": self.is_final,
            "peer_reachable_count": self.peer_reachable_count,
        }
        if self.output_preview is not None:
            result["output_preview"] = self.output_preview
        if self.completion_cursor is not None:
            result["completion_cursor"] = self.completion_cursor.to_dict()
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> IdentityInspection:
        return cls(
            identity=data["identity"],
            output_preview=data.get("output_preview"),
            is_final=bool(data.get("is_final", False)),
            peer_reachable_count=int(data.get("peer_reachable_count", 0)),
            completion_cursor=_completion_cursor_from(data, "completion_cursor"),
        )


@dataclass(frozen=True)
class ConsoleIdentityRecord:
    """One roster member from ``mobkit/console/list_identities``.

    The typed identity -> (session, profile) map: ``session_id`` is the
    member's current durable binding and the adopted roster profile rides
    ``labels`` (``labels["role"]`` in the shipped gateways). This is the
    supported replacement for reading the continuity store directly.
    """

    identity: str
    display_name: str
    runtime_key: str
    runtime_member_id: str
    visibility: str
    addressable: bool
    health: str
    session_id: str | None = None
    topology_peers: list[str] = field(default_factory=list)
    labels: dict[str, str] = field(default_factory=dict)

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ConsoleIdentityRecord:
        return cls(
            identity=data["identity"],
            display_name=str(data.get("display_name", "")),
            runtime_key=str(data.get("runtime_key", "")),
            runtime_member_id=str(data.get("runtime_member_id", "")),
            visibility=str(data.get("visibility", "")),
            addressable=bool(data.get("addressable", False)),
            health=str(data.get("health", "")),
            session_id=data.get("session_id"),
            topology_peers=[str(peer) for peer in data.get("topology_peers", [])],
            labels={str(k): str(v) for k, v in (data.get("labels") or {}).items()},
        )
