"""Typed return models for MobKit SDK RPC methods."""
from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any


@dataclass(frozen=True)
class StatusResult:
    contract_version: str
    running: bool
    loaded_modules: list[str]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> StatusResult:
        return cls(
            contract_version=data["contract_version"],
            running=data["running"],
            loaded_modules=list(data.get("loaded_modules", [])),
        )


@dataclass(frozen=True)
class CapabilitiesResult:
    contract_version: str
    methods: list[str]
    loaded_modules: list[str]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> CapabilitiesResult:
        return cls(
            contract_version=data["contract_version"],
            methods=list(data.get("methods", [])),
            loaded_modules=list(data.get("loaded_modules", [])),
        )


@dataclass(frozen=True)
class ReconcileResult:
    accepted: bool
    reconciled_modules: list[str]
    added: int

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ReconcileResult:
        return cls(
            accepted=data["accepted"],
            reconciled_modules=list(data.get("reconciled_modules", [])),
            added=data["added"],
        )


@dataclass(frozen=True)
class SpawnResult:
    """Result of spawning a mob member (both spec-based and module-id-based)."""
    accepted: bool
    module_id: str
    meerkat_id: str | None = None
    profile: str | None = None

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> SpawnResult:
        return cls(
            accepted=data["accepted"],
            module_id=data.get("module_id", ""),
            meerkat_id=data.get("meerkat_id"),
            profile=data.get("profile"),
        )


# Alias for backward compat within SDK — both spawn paths return SpawnResult
SpawnMemberResult = SpawnResult


@dataclass(frozen=True)
class KeepAliveConfig:
    interval_ms: int
    event: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> KeepAliveConfig:
        return cls(
            interval_ms=data.get("interval_ms", 0),
            event=data.get("event", ""),
        )


@dataclass(frozen=True)
class EventEnvelope:
    event_id: str
    source: str
    timestamp_ms: int
    event: Any

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> EventEnvelope:
        return cls(
            event_id=data.get("event_id", ""),
            source=data.get("source", ""),
            timestamp_ms=data.get("timestamp_ms", 0),
            event=data.get("event"),
        )


@dataclass(frozen=True)
class SubscribeResult:
    scope: str
    replay_from_event_id: str | None
    keep_alive: KeepAliveConfig
    keep_alive_comment: str
    event_frames: list[str]
    events: list[EventEnvelope]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> SubscribeResult:
        ka_raw = data.get("keep_alive", {})
        events_raw = data.get("events", [])
        return cls(
            scope=data["scope"],
            replay_from_event_id=data.get("replay_from_event_id"),
            keep_alive=KeepAliveConfig.from_dict(ka_raw),
            keep_alive_comment=data.get("keep_alive_comment", ""),
            event_frames=list(data.get("event_frames", [])),
            events=[EventEnvelope.from_dict(e) for e in events_raw],
        )


@dataclass(frozen=True)
class SendMessageResult:
    accepted: bool
    member_id: str
    session_id: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> SendMessageResult:
        return cls(
            accepted=data.get("accepted", False),
            member_id=data.get("member_id", ""),
            session_id=data.get("session_id", ""),
        )


@dataclass(frozen=True)
class RoutingResolution:
    recipient: str
    route: dict[str, Any]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> RoutingResolution:
        return cls(
            recipient=data.get("recipient", ""),
            route=dict(data.get("route", data)),
        )


@dataclass(frozen=True)
class DeliveryResult:
    delivered: bool
    delivery_id: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> DeliveryResult:
        return cls(
            delivered=data.get("delivered", False),
            delivery_id=data.get("delivery_id", ""),
        )


@dataclass(frozen=True)
class MemoryQueryResult:
    results: list[dict[str, Any]]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MemoryQueryResult:
        return cls(results=list(data.get("results", [])))


@dataclass(frozen=True)
class CallToolResult:
    """Result of calling an MCP tool on a loaded module."""
    module_id: str
    tool: str
    result: Any

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> CallToolResult:
        return cls(
            module_id=data.get("module_id", ""),
            tool=data.get("tool", ""),
            result=data.get("result"),
        )


@dataclass(frozen=True)
class RediscoverReport:
    """Report from a rediscover operation (reset + re-run discovery)."""
    spawned: list[str]
    edges: ReconcileEdgesReport

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> RediscoverReport:
        return cls(
            spawned=list(data.get("spawned", [])),
            edges=ReconcileEdgesReport.from_dict(data.get("edges", {})),
        )


@dataclass(frozen=True)
class ReconcileEdgesReport:
    """Report from dynamic edge reconciliation."""
    desired_edges: list[dict[str, Any]]
    wired_edges: list[dict[str, Any]]
    unwired_edges: list[dict[str, Any]]
    retained_edges: list[dict[str, Any]]
    preexisting_edges: list[dict[str, Any]]
    skipped_missing_members: list[dict[str, Any]]
    pruned_stale_managed_edges: list[dict[str, Any]]
    failures: list[dict[str, Any]]

    @property
    def is_complete(self) -> bool:
        return not self.failures and not self.skipped_missing_members

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ReconcileEdgesReport:
        return cls(
            desired_edges=list(data.get("desired_edges", [])),
            wired_edges=list(data.get("wired_edges", [])),
            unwired_edges=list(data.get("unwired_edges", [])),
            retained_edges=list(data.get("retained_edges", [])),
            preexisting_edges=list(data.get("preexisting_edges", [])),
            skipped_missing_members=list(data.get("skipped_missing_members", [])),
            pruned_stale_managed_edges=list(data.get("pruned_stale_managed_edges", [])),
            failures=list(data.get("failures", [])),
        )


@dataclass(frozen=True)
class UnifiedAgentEvent:
    """An agent event reference from the unified event bus."""
    agent_id: str
    event_type: str


@dataclass(frozen=True)
class UnifiedModuleEvent:
    """A module event from the unified event bus."""
    module: str
    event_type: str
    payload: dict[str, Any] = field(default_factory=dict)


# Union of both event kinds
UnifiedEvent = UnifiedAgentEvent | UnifiedModuleEvent


def _parse_unified_event(raw: dict[str, Any]) -> UnifiedEvent:
    """Parse a serialized UnifiedEvent (externally tagged Rust enum)."""
    if "Agent" in raw:
        agent = raw["Agent"]
        return UnifiedAgentEvent(
            agent_id=agent.get("agent_id", ""),
            event_type=agent.get("event_type", ""),
        )
    if "Module" in raw:
        module = raw["Module"]
        return UnifiedModuleEvent(
            module=module.get("module", ""),
            event_type=module.get("event_type", ""),
            payload=module.get("payload", {}),
        )
    # Fallback for unknown shapes
    return UnifiedModuleEvent(module="unknown", event_type="unknown", payload=raw)


@dataclass(frozen=True)
class PersistedEvent:
    """A persisted operational event with monotonic ordering."""
    id: str
    seq: int
    timestamp_ms: int
    member_id: str | None
    event: UnifiedEvent

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> PersistedEvent:
        raw_event = data.get("event", {})
        event = _parse_unified_event(raw_event) if isinstance(raw_event, dict) else UnifiedModuleEvent(module="unknown", event_type="unknown", payload={})
        return cls(
            id=data["id"],
            seq=data["seq"],
            timestamp_ms=data["timestamp_ms"],
            member_id=data.get("member_id"),
            event=event,
        )


@dataclass
class EventQuery:
    """Query parameters for historical event retrieval."""
    since_ms: int | None = None
    until_ms: int | None = None
    member_id: str | None = None
    event_types: list[str] = field(default_factory=list)
    limit: int | None = None
    after_seq: int | None = None

    def to_dict(self) -> dict[str, Any]:
        d: dict[str, Any] = {}
        if self.since_ms is not None:
            d["since_ms"] = self.since_ms
        if self.until_ms is not None:
            d["until_ms"] = self.until_ms
        if self.member_id is not None:
            d["member_id"] = self.member_id
        if self.event_types:
            d["event_types"] = self.event_types
        if self.limit is not None:
            d["limit"] = self.limit
        if self.after_seq is not None:
            d["after_seq"] = self.after_seq
        return d


MEMBER_STATE_ACTIVE: str = "active"
MEMBER_STATE_RETIRING: str = "retiring"


@dataclass(frozen=True)
class MemberSnapshot:
    """Snapshot of a mob member from the roster.

    The ``state`` field is one of :data:`MEMBER_STATE_ACTIVE` or
    :data:`MEMBER_STATE_RETIRING`.
    """
    meerkat_id: str
    profile: str
    state: str
    wired_to: list[str]
    labels: dict[str, str]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MemberSnapshot:
        return cls(
            meerkat_id=data["meerkat_id"],
            profile=data["profile"],
            state=data["state"],
            wired_to=list(data.get("wired_to", [])),
            labels=dict(data.get("labels", {})),
        )


@dataclass(frozen=True)
class RuntimeRouteResult:
    """A runtime route entry."""
    route_key: str
    recipient: str
    channel: str | None
    sink: str
    target_module: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> RuntimeRouteResult:
        return cls(
            route_key=data["route_key"],
            recipient=data["recipient"],
            channel=data.get("channel"),
            sink=data["sink"],
            target_module=data["target_module"],
        )


@dataclass(frozen=True)
class DeliveryHistoryResult:
    """Result of a delivery history query."""
    deliveries: list[dict[str, Any]]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> DeliveryHistoryResult:
        return cls(deliveries=list(data.get("deliveries", [])))


@dataclass(frozen=True)
class GatingEvaluateResult:
    """Result of a gating evaluation."""
    action_id: str
    action: str
    actor_id: str
    risk_tier: str | None
    outcome: str
    pending_id: str | None

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> GatingEvaluateResult:
        return cls(
            action_id=data["action_id"],
            action=data["action"],
            actor_id=data["actor_id"],
            risk_tier=data.get("risk_tier"),
            outcome=data["outcome"],
            pending_id=data.get("pending_id"),
        )


@dataclass(frozen=True)
class GatingDecisionResult:
    """Result of a gating decision."""
    pending_id: str
    action_id: str
    decision: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> GatingDecisionResult:
        return cls(
            pending_id=data["pending_id"],
            action_id=data["action_id"],
            decision=data["decision"],
        )


@dataclass(frozen=True)
class GatingAuditEntry:
    """An entry in the gating audit log."""
    audit_id: str
    timestamp_ms: int
    event_type: str
    action_id: str
    actor_id: str
    risk_tier: str | None
    outcome: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> GatingAuditEntry:
        return cls(
            audit_id=data["audit_id"],
            timestamp_ms=data["timestamp_ms"],
            event_type=data["event_type"],
            action_id=data["action_id"],
            actor_id=data["actor_id"],
            risk_tier=data.get("risk_tier"),
            outcome=data["outcome"],
        )


@dataclass(frozen=True)
class GatingPendingEntry:
    """A pending gating decision awaiting approval."""
    pending_id: str
    action_id: str
    action: str
    actor_id: str
    risk_tier: str | None
    created_at_ms: int

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> GatingPendingEntry:
        return cls(
            pending_id=data["pending_id"],
            action_id=data["action_id"],
            action=data["action"],
            actor_id=data["actor_id"],
            risk_tier=data.get("risk_tier"),
            created_at_ms=data.get("created_at_ms", 0),
        )


@dataclass(frozen=True)
class MemoryStoreInfo:
    """Information about a memory store."""
    store: str
    record_count: int

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MemoryStoreInfo:
        return cls(
            store=data["store"],
            record_count=data.get("record_count", 0),
        )


@dataclass(frozen=True)
class MemoryIndexResult:
    """Result of a memory index operation."""
    entity: str
    topic: str
    store: str
    assertion_id: str | None

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MemoryIndexResult:
        return cls(
            entity=data["entity"],
            topic=data["topic"],
            store=data["store"],
            assertion_id=data.get("assertion_id"),
        )


@dataclass(frozen=True)
class CatalogEntry:
    """A curated model entry from the model catalog."""
    id: str
    display_name: str
    provider: str
    tier: str
    context_window: int | None = None
    max_output_tokens: int | None = None

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> CatalogEntry:
        return cls(
            id=data["id"],
            display_name=data["display_name"],
            provider=data["provider"],
            tier=data["tier"],
            context_window=data.get("context_window"),
            max_output_tokens=data.get("max_output_tokens"),
        )


@dataclass(frozen=True)
class ProviderDefaults:
    """Provider-level grouping with a default model."""
    provider: str
    default_model_id: str
    models: list[CatalogEntry]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ProviderDefaults:
        return cls(
            provider=data["provider"],
            default_model_id=data["default_model_id"],
            models=[CatalogEntry.from_dict(m) for m in data.get("models", [])],
        )


@dataclass(frozen=True)
class ModelsCatalogResult:
    """Result of a models/catalog RPC call."""
    models: list[CatalogEntry]
    provider_defaults: list[ProviderDefaults]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ModelsCatalogResult:
        return cls(
            models=[CatalogEntry.from_dict(m) for m in data.get("models", [])],
            provider_defaults=[
                ProviderDefaults.from_dict(p) for p in data.get("provider_defaults", [])
            ],
        )


class ErrorCategory(str, Enum):
    """Error event categories matching Rust's ErrorEvent variants."""
    SPAWN_FAILURE = "spawn_failure"
    RECONCILE_INCOMPLETE = "reconcile_incomplete"
    CHECKPOINT_FAILURE = "checkpoint_failure"
    HOST_LOOP_CRASH = "host_loop_crash"
    REDISCOVER_FAILURE = "rediscover_failure"


@dataclass(frozen=True)
class ErrorEvent:
    """Operational error event for alerting.

    Matches Rust's ``ErrorEvent`` enum. The ``category`` field corresponds
    to the enum variant, and ``context`` carries the variant's fields.

    Usage::

        async def on_error(event: ErrorEvent):
            if event.category == ErrorCategory.SPAWN_FAILURE:
                member_id = event.context["member_id"]
                await alerts.send(f"spawn failed: {member_id}: {event.message}")
    """
    category: str
    message: str
    context: dict[str, Any] = field(default_factory=dict)

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ErrorEvent:
        category = data.get("category", "unknown")
        context = {k: v for k, v in data.items() if k != "category"}
        # Build a human-readable message from the context
        error = context.get("error", "")
        member_id = context.get("member_id", "")
        if category == ErrorCategory.SPAWN_FAILURE:
            message = f"{member_id}: {error}" if member_id else error
        elif category == ErrorCategory.RECONCILE_INCOMPLETE:
            failures = context.get("failures", 0)
            skipped = context.get("skipped", 0)
            message = f"{failures} failures, {skipped} skipped"
        elif category == ErrorCategory.CHECKPOINT_FAILURE:
            session_id = context.get("session_id", "")
            message = f"{session_id}: {error}" if session_id else error
        elif category == ErrorCategory.HOST_LOOP_CRASH:
            message = f"{member_id}: {error}" if member_id else error
        elif category == ErrorCategory.REDISCOVER_FAILURE:
            message = error
        else:
            message = str(data)
        return cls(category=category, message=message, context=context)
