"""Typed return models for MobKit SDK RPC methods."""
from __future__ import annotations

from dataclasses import dataclass, field
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
