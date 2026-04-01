"""Provider protocols and result models for identity-first continuity (REQ-42, REQ-43c)."""
from __future__ import annotations

import base64
from dataclasses import dataclass
from typing import Any, Protocol, runtime_checkable


# ---------------------------------------------------------------------------
# Result models (REQ-43c)
# ---------------------------------------------------------------------------

@dataclass(frozen=True)
class ContinuityRecord:
    identity: str
    agent_runtime_id: str
    session_id: str
    generation: int
    checkpoint_version: int

    def to_dict(self) -> dict[str, Any]:
        return {
            "identity": self.identity,
            "agent_runtime_id": self.agent_runtime_id,
            "session_id": self.session_id,
            "generation": self.generation,
            "checkpoint_version": self.checkpoint_version,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ContinuityRecord:
        return cls(
            identity=data["identity"],
            agent_runtime_id=data["agent_runtime_id"],
            session_id=data["session_id"],
            generation=data["generation"],
            checkpoint_version=data["checkpoint_version"],
        )


@dataclass(frozen=True)
class ContinuityFailure:
    identity: str
    kind: str
    record: ContinuityRecord | None
    detail: str

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {
            "identity": self.identity,
            "kind": self.kind,
            "detail": self.detail,
        }
        result["record"] = self.record.to_dict() if self.record else None
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ContinuityFailure:
        rec_raw = data.get("record")
        return cls(
            identity=data["identity"],
            kind=data["kind"],
            record=ContinuityRecord.from_dict(rec_raw) if rec_raw else None,
            detail=data["detail"],
        )


@dataclass
class ContinuityResolveState:
    """Tagged union: state is 'uninitialized', 'ready', or 'broken'."""

    state: str
    record: ContinuityRecord | None = None
    failure: ContinuityFailure | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {"state": self.state}
        if self.record is not None:
            result["record"] = self.record.to_dict()
        if self.failure is not None:
            result["failure"] = self.failure.to_dict()
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ContinuityResolveState:
        rec_raw = data.get("record")
        fail_raw = data.get("failure")
        return cls(
            state=data["state"],
            record=ContinuityRecord.from_dict(rec_raw) if rec_raw else None,
            failure=ContinuityFailure.from_dict(fail_raw) if fail_raw else None,
        )


@dataclass(frozen=True)
class SessionSnapshot:
    """Opaque session snapshot. data is bytes; wire format is base64."""

    data: bytes

    def to_dict(self) -> dict[str, Any]:
        return {"data": base64.b64encode(self.data).decode()}

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> SessionSnapshot:
        return cls(data=base64.b64decode(data["data"]))


@dataclass(frozen=True)
class LeaseGrant:
    identity: str
    fencing_token: int
    ttl_ms: int

    def to_dict(self) -> dict[str, Any]:
        return {
            "identity": self.identity,
            "fencing_token": self.fencing_token,
            "ttl_ms": self.ttl_ms,
        }

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> LeaseGrant:
        return cls(
            identity=data["identity"],
            fencing_token=data["fencing_token"],
            ttl_ms=data["ttl_ms"],
        )


@dataclass
class LeaseAcquireResult:
    """Tagged union: status is 'acquired' or 'already_held'."""

    status: str
    grant: LeaseGrant | None = None
    holder: str | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {"status": self.status}
        if self.grant is not None:
            result["grant"] = self.grant.to_dict()
        if self.holder is not None:
            result["holder"] = self.holder
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> LeaseAcquireResult:
        grant_raw = data.get("grant")
        return cls(
            status=data["status"],
            grant=LeaseGrant.from_dict(grant_raw) if grant_raw else None,
            holder=data.get("holder"),
        )


@dataclass
class LeaseRenewResult:
    """Tagged union: status is 'renewed' or 'lost'."""

    status: str
    grant: LeaseGrant | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {"status": self.status}
        if self.grant is not None:
            result["grant"] = self.grant.to_dict()
        return result

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> LeaseRenewResult:
        grant_raw = data.get("grant")
        return cls(
            status=data["status"],
            grant=LeaseGrant.from_dict(grant_raw) if grant_raw else None,
        )


# ---------------------------------------------------------------------------
# Provider protocols (REQ-42)
# ---------------------------------------------------------------------------

@runtime_checkable
class ContinuityStoreProvider(Protocol):
    async def resolve_many(
        self, identities: list[str],
    ) -> dict[str, ContinuityResolveState]: ...

    async def load_session_snapshot(
        self, session_id: str,
    ) -> SessionSnapshot | None: ...

    async def save_session_snapshot(
        self, identity: str, session_id: str, generation: int,
        version: int, fencing_token: int, snapshot: SessionSnapshot,
    ) -> None: ...

    async def upsert_continuity_record(
        self, record: ContinuityRecord, fencing_token: int,
    ) -> None: ...


@runtime_checkable
class LeaseProviderProtocol(Protocol):
    async def acquire_leases(
        self, identities: list[str], runtime_instance: str,
    ) -> dict[str, LeaseAcquireResult]: ...

    async def renew_leases(
        self, grants: list[LeaseGrant],
    ) -> dict[str, LeaseRenewResult]: ...

    async def release_leases(
        self, grants: list[LeaseGrant],
    ) -> None: ...


@runtime_checkable
class RosterProviderProtocol(Protocol):
    async def roster(self, context: Any) -> list[Any]: ...


@runtime_checkable
class AgentCustomizerProtocol(Protocol):
    async def customize_build(
        self, context: Any, spec: Any, draft: Any,
    ) -> None: ...

    async def after_create(
        self, identity: str, session_id: str, context: Any,
    ) -> None: ...


@runtime_checkable
class TopologyProviderProtocol(Protocol):
    async def compute_edges(
        self, target_identities: list[str], context: Any,
    ) -> list[Any]: ...
