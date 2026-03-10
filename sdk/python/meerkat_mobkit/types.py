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
class PersistedEvent:
    """A persisted operational event with monotonic ordering."""
    id: str
    seq: int
    timestamp_ms: int
    member_id: str | None
    event: dict[str, Any]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> PersistedEvent:
        return cls(
            id=data["id"],
            seq=data["seq"],
            timestamp_ms=data["timestamp_ms"],
            member_id=data.get("member_id"),
            event=data.get("event", {}),
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
        if category == "spawn_failure":
            message = f"{member_id}: {error}" if member_id else error
        elif category == "reconcile_incomplete":
            failures = context.get("failures", 0)
            skipped = context.get("skipped", 0)
            message = f"{failures} failures, {skipped} skipped"
        elif category == "checkpoint_failure":
            session_id = context.get("session_id", "")
            message = f"{session_id}: {error}" if session_id else error
        elif category == "host_loop_crash":
            message = f"{member_id}: {error}" if member_id else error
        elif category == "rediscover_failure":
            message = error
        else:
            message = str(data)
        return cls(category=category, message=message, context=context)
