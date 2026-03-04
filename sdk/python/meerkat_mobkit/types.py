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
    accepted: bool
    module_id: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> SpawnResult:
        return cls(accepted=data["accepted"], module_id=data["module_id"])


@dataclass(frozen=True)
class SpawnMemberResult:
    accepted: bool
    module_id: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> SpawnMemberResult:
        return cls(accepted=data["accepted"], module_id=data["module_id"])


@dataclass(frozen=True)
class SubscribeResult:
    scope: str
    replay_from_event_id: str | None
    keep_alive: dict[str, Any]
    keep_alive_comment: str
    event_frames: list[str]
    events: list[dict[str, Any]]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> SubscribeResult:
        return cls(
            scope=data["scope"],
            replay_from_event_id=data.get("replay_from_event_id"),
            keep_alive=dict(data.get("keep_alive", {})),
            keep_alive_comment=data.get("keep_alive_comment", ""),
            event_frames=list(data.get("event_frames", [])),
            events=list(data.get("events", [])),
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
