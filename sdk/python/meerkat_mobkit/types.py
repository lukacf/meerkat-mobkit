"""Typed return models for MobKit SDK RPC methods."""
from __future__ import annotations

from dataclasses import dataclass, field
from enum import Enum
from typing import Any


def _coerce_int(value: Any, default: int = 0) -> int:
    """Best-effort int coercion for wire-typed dataclass fields.

    The wire schema documents these fields as JSON numbers, but stubs,
    older gateways, and test fixtures sometimes deliver `None`, `"15"`,
    or floats. Pre-fix the SDK's `from_dict` parsers passed these
    through untouched, breaking downstream arithmetic far from the
    parse site. The fallback is `default` (typically `0`).
    """
    if value is None:
        return default
    if isinstance(value, bool):
        return int(value)
    if isinstance(value, int):
        return value
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


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
class ProfileCapabilities:
    instance_count: int
    addressable: bool
    has_wiring: bool

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> ProfileCapabilities:
        return cls(
            instance_count=int(data.get("instance_count", 0)),
            addressable=bool(data.get("addressable", True)),
            has_wiring=bool(data.get("has_wiring", False)),
        )


@dataclass(frozen=True)
class RuntimeCapabilities:
    can_spawn_members: bool
    can_send_messages: bool
    can_wire_members: bool
    can_retire_members: bool
    available_spawn_modes: list[str]
    profile_capabilities: dict[str, ProfileCapabilities] = field(default_factory=dict)

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> RuntimeCapabilities:
        pc_raw = data.get("profile_capabilities", {})
        return cls(
            can_spawn_members=bool(data.get("can_spawn_members", False)),
            can_send_messages=bool(data.get("can_send_messages", False)),
            can_wire_members=bool(data.get("can_wire_members", False)),
            can_retire_members=bool(data.get("can_retire_members", False)),
            available_spawn_modes=list(data.get("available_spawn_modes", [])),
            profile_capabilities={
                k: ProfileCapabilities.from_dict(v) for k, v in pc_raw.items()
            } if isinstance(pc_raw, dict) else {},
        )


@dataclass(frozen=True)
class CapabilitiesResult:
    contract_version: str
    methods: list[str]
    loaded_modules: list[str]
    runtime_capabilities: RuntimeCapabilities | None = None

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> CapabilitiesResult:
        rc_raw = data.get("runtime_capabilities")
        return cls(
            contract_version=data["contract_version"],
            methods=list(data.get("methods", [])),
            loaded_modules=list(data.get("loaded_modules", [])),
            runtime_capabilities=RuntimeCapabilities.from_dict(rc_raw) if rc_raw else None,
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
    agent_identity: str | None = None
    role: str | None = None

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> SpawnResult:
        return cls(
            accepted=data["accepted"],
            module_id=data.get("module_id", ""),
            agent_identity=data.get("agent_identity"),
            role=data.get("role"),
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
            interval_ms=_coerce_int(data.get("interval_ms"), 0),
            event=str(data.get("event", "")),
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
            event_id=str(data.get("event_id", "")),
            source=str(data.get("source", "")),
            timestamp_ms=_coerce_int(data.get("timestamp_ms"), 0),
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
    """Parse a serialized UnifiedEvent.

    Rust currently emits internally tagged events, e.g.
    ``{"kind": "agent", "agent_id": "..."}``. The older externally tagged
    shape is still accepted for compatibility with pre-0.6 fixtures.
    """
    if raw.get("kind") == "agent":
        return UnifiedAgentEvent(
            agent_id=raw.get("agent_id", raw.get("agentId", "")),
            event_type=raw.get("event_type", raw.get("eventType", "")),
        )
    if raw.get("kind") == "module":
        return UnifiedModuleEvent(
            module=raw.get("module", ""),
            event_type=raw.get("event_type", raw.get("eventType", "")),
            payload=raw.get("payload", {}),
        )
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
    """Query parameters for historical event retrieval.

    ``after_seq`` is the pagination cursor — pass the highest ``seq`` /
    ``cursor`` you have seen to receive only strictly-newer events.
    ``mob_id``, ``run_id``, ``step_id``, and ``identity`` filter the
    structural mob-events surface (``mobkit/mob_events/query``).
    """
    since_ms: int | None = None
    until_ms: int | None = None
    member_id: str | None = None
    identity: str | None = None
    mob_id: str | None = None
    run_id: str | None = None
    step_id: str | None = None
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
        if self.identity is not None:
            d["identity"] = self.identity
        if self.mob_id is not None:
            d["mob_id"] = self.mob_id
        if self.run_id is not None:
            d["run_id"] = self.run_id
        if self.step_id is not None:
            d["step_id"] = self.step_id
        if self.event_types:
            d["event_types"] = self.event_types
        if self.limit is not None:
            d["limit"] = self.limit
        if self.after_seq is not None:
            d["after_seq"] = self.after_seq
        return d


@dataclass(frozen=True)
class MobStructuralEvent:
    """A structural mob event projected from ``MobEventKind``.

    Carries flow/step/identity context that the legacy lossy
    ``UnifiedEvent::Agent`` projection discards. Use ``cursor`` as the
    ``EventQuery.after_seq`` pagination token on the next request.
    """
    event_id: str
    cursor: int
    mob_id: str
    timestamp_ms: int
    kind: str
    run_id: str | None
    step_id: str | None
    agent_identity: str | None
    mob_labels: dict[str, str]
    run_labels: dict[str, str]
    data: dict[str, Any]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MobStructuralEvent:
        payload = data.get("data") if isinstance(data.get("data"), dict) else {}
        mob_labels_raw = data.get("mob_labels", {})
        run_labels_raw = data.get("run_labels", {})
        return cls(
            event_id=str(data.get("event_id", "")),
            cursor=int(data.get("cursor", 0)),
            mob_id=str(data.get("mob_id", "")),
            timestamp_ms=int(data.get("timestamp_ms", 0)),
            kind=str(data.get("kind", "")),
            run_id=data.get("run_id"),
            step_id=data.get("step_id"),
            agent_identity=data.get("agent_identity"),
            mob_labels=dict(mob_labels_raw) if isinstance(mob_labels_raw, dict) else {},
            run_labels=dict(run_labels_raw) if isinstance(run_labels_raw, dict) else {},
            data=dict(payload) if isinstance(payload, dict) else {},
        )


class MobRunStatus(str, Enum):
    """Lifecycle states for a flow run.

    Mirrors meerkat's ``MobRunStatus`` enum (snake_case wire form).
    """

    PENDING = "pending"
    RUNNING = "running"
    COMPLETED = "completed"
    FAILED = "failed"
    CANCELED = "canceled"

    @classmethod
    def parse(cls, raw: Any) -> MobRunStatus:
        if isinstance(raw, MobRunStatus):
            return raw
        if isinstance(raw, str):
            try:
                return cls(raw)
            except ValueError:
                return cls.PENDING
        return cls.PENDING


@dataclass(frozen=True)
class StepRecord:
    """Per-target step execution ledger entry.

    Mirrors meerkat's ``StepLedgerEntry``. ``status`` is a snake_case
    string (e.g. ``"dispatched"``, ``"completed"``, ``"failed"``).
    """
    step_id: str
    agent_identity: str
    status: str
    output: Any
    timestamp: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> StepRecord:
        return cls(
            step_id=str(data.get("step_id", "")),
            agent_identity=str(data.get("agent_identity", "")),
            status=str(data.get("status", "")),
            output=data.get("output"),
            timestamp=str(data.get("timestamp", "")),
        )


@dataclass(frozen=True)
class FailureRecord:
    """Flow-level failure log entry. Mirrors meerkat's ``FailureLedgerEntry``."""
    step_id: str
    reason: str
    timestamp: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> FailureRecord:
        return cls(
            step_id=str(data.get("step_id", "")),
            reason=str(data.get("reason", "")),
            timestamp=str(data.get("timestamp", "")),
        )


@dataclass(frozen=True)
class FrameRecord:
    """Per-frame kernel snapshot. Mirrors meerkat's ``FrameSnapshot``.

    The ``kernel_state`` shape is meerkat-internal flow_frame state and
    passes through as :class:`Any`.
    """
    kernel_state: Any

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> FrameRecord:
        return cls(kernel_state=data.get("kernel_state"))


@dataclass(frozen=True)
class LoopRecord:
    """Per-loop kernel snapshot. Mirrors meerkat's ``LoopSnapshot``.

    The ``kernel_state`` shape is meerkat-internal loop_iteration state
    and passes through as :class:`Any`.
    """
    kernel_state: Any

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> LoopRecord:
        return cls(kernel_state=data.get("kernel_state"))


@dataclass(frozen=True)
class LoopIterationRecord:
    """Loop-iteration → body-frame ledger entry.

    Mirrors meerkat's ``LoopIterationLedgerEntry``.
    """
    loop_instance_id: str
    iteration: int
    frame_id: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> LoopIterationRecord:
        return cls(
            loop_instance_id=str(data.get("loop_instance_id", "")),
            iteration=int(data.get("iteration", 0)),
            frame_id=str(data.get("frame_id", "")),
        )


@dataclass(frozen=True)
class MobRun:
    """Persisted flow run aggregate returned by :meth:`MobHandle.list_runs`.

    Carries the full meerkat ledger projection. Meerkat-internal
    sub-shapes (``flow_state``, ``activation_params``,
    ``StepRecord.output``, ``root_step_outputs`` /
    ``loop_iteration_outputs`` value blobs, frame / loop
    ``kernel_state``) pass through as :class:`Any` rather than being
    re-typed in the SDK.
    """
    run_id: str
    mob_id: str
    flow_id: str
    status: MobRunStatus
    flow_state: Any
    activation_params: Any
    created_at: str
    completed_at: str | None
    step_ledger: list[StepRecord]
    failure_ledger: list[FailureRecord]
    frames: dict[str, FrameRecord]
    loops: dict[str, LoopRecord]
    loop_iteration_ledger: list[LoopIterationRecord]
    schema_version: int
    root_step_outputs: dict[str, Any]
    loop_iteration_outputs: dict[str, Any]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MobRun:
        step_raw = data.get("step_ledger", [])
        failure_raw = data.get("failure_ledger", [])
        frames_raw = data.get("frames", {})
        loops_raw = data.get("loops", {})
        iter_raw = data.get("loop_iteration_ledger", [])
        completed = data.get("completed_at")
        return cls(
            run_id=str(data.get("run_id", "")),
            mob_id=str(data.get("mob_id", "")),
            flow_id=str(data.get("flow_id", "")),
            status=MobRunStatus.parse(data.get("status")),
            flow_state=data.get("flow_state"),
            activation_params=data.get("activation_params"),
            created_at=str(data.get("created_at", "")),
            completed_at=str(completed) if completed is not None else None,
            step_ledger=[
                StepRecord.from_dict(entry)
                for entry in step_raw
                if isinstance(entry, dict)
            ],
            failure_ledger=[
                FailureRecord.from_dict(entry)
                for entry in failure_raw
                if isinstance(entry, dict)
            ],
            frames={
                str(key): FrameRecord.from_dict(value)
                for key, value in (frames_raw.items() if isinstance(frames_raw, dict) else [])
                if isinstance(value, dict)
            },
            loops={
                str(key): LoopRecord.from_dict(value)
                for key, value in (loops_raw.items() if isinstance(loops_raw, dict) else [])
                if isinstance(value, dict)
            },
            loop_iteration_ledger=[
                LoopIterationRecord.from_dict(entry)
                for entry in iter_raw
                if isinstance(entry, dict)
            ],
            schema_version=int(data.get("schema_version", 0)),
            root_step_outputs=dict(data.get("root_step_outputs", {}))
            if isinstance(data.get("root_step_outputs"), dict)
            else {},
            loop_iteration_outputs=dict(data.get("loop_iteration_outputs", {}))
            if isinstance(data.get("loop_iteration_outputs"), dict)
            else {},
        )


MEMBER_STATE_ACTIVE: str = "active"
MEMBER_STATE_RETIRING: str = "retiring"


@dataclass(frozen=True)
class MemberSnapshot:
    """Snapshot of a mob member from the roster.

    The ``state`` field is one of :data:`MEMBER_STATE_ACTIVE` or
    :data:`MEMBER_STATE_RETIRING`.
    """
    agent_identity: str
    role: str
    state: str
    wired_to: list[str]
    labels: dict[str, str]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MemberSnapshot:
        return cls(
            agent_identity=data["agent_identity"],
            role=data["role"],
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
class CrossMobContactEntry:
    """An entry in the cross-mob contact directory."""
    mob_id: str
    transport: str

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> CrossMobContactEntry:
        transport = data.get("transport", "")
        if isinstance(transport, dict):
            if "Tcp" in transport:
                transport = f"tcp://{transport['Tcp']}"
            elif "Uds" in transport:
                transport = f"uds://{transport['Uds']}"
            else:
                transport = "inproc"
        elif transport == "Inproc":
            transport = "inproc"
        return cls(mob_id=data.get("mob_id", ""), transport=transport)


@dataclass(frozen=True)
class CatalogEntry:
    """A curated model entry from the model catalog."""
    id: str
    display_name: str
    provider: str
    tier: str
    context_window: int | None = None
    max_output_tokens: int | None = None
    vision: bool = False
    image_tool_results: bool = False

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> CatalogEntry:
        profile = data.get("profile", {})
        return cls(
            id=data["id"],
            display_name=data["display_name"],
            provider=data["provider"],
            tier=data["tier"],
            context_window=data.get("context_window"),
            max_output_tokens=data.get("max_output_tokens"),
            vision=profile.get("vision", False),
            image_tool_results=profile.get("image_tool_results", False),
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


@dataclass(frozen=True)
class MobpackToolsCatalogResult:
    """Result of a mobkit/tools/catalog RPC call."""
    schema_version: str
    runtime_backed: bool
    source: str
    authoring_provider: dict[str, Any]
    runtime_unavailable_reason: str | None
    tool_catalog: list[dict[str, Any]]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MobpackToolsCatalogResult:
        return cls(
            schema_version=str(data.get("schema_version", "")),
            runtime_backed=bool(data.get("runtime_backed", False)),
            source=str(data.get("source", "")),
            authoring_provider=dict(data.get("authoring_provider", {})),
            runtime_unavailable_reason=data.get("runtime_unavailable_reason"),
            tool_catalog=list(data.get("tool_catalog", [])),
        )


@dataclass(frozen=True)
class MobpackSkillsCatalogResult:
    """Result of a mobkit/skills/catalog RPC call."""
    schema_version: str
    runtime_backed: bool
    source: str
    authoring_provider: dict[str, Any]
    runtime_unavailable_reason: str | None
    skill_realms: list[dict[str, Any]]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MobpackSkillsCatalogResult:
        return cls(
            schema_version=str(data.get("schema_version", "")),
            runtime_backed=bool(data.get("runtime_backed", False)),
            source=str(data.get("source", "")),
            authoring_provider=dict(data.get("authoring_provider", {})),
            runtime_unavailable_reason=data.get("runtime_unavailable_reason"),
            skill_realms=list(data.get("skill_realms", [])),
        )


@dataclass(frozen=True)
class MobpackAgentDefinitionsResult:
    """Result of a mobkit/agent_definitions/list RPC call."""
    schema_version: str
    runtime_backed: bool
    source: str
    authoring_provider: dict[str, Any]
    runtime_unavailable_reason: str | None
    agent_definitions: list[dict[str, Any]]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MobpackAgentDefinitionsResult:
        return cls(
            schema_version=str(data.get("schema_version", "")),
            runtime_backed=bool(data.get("runtime_backed", False)),
            source=str(data.get("source", "")),
            authoring_provider=dict(data.get("authoring_provider", {})),
            runtime_unavailable_reason=data.get("runtime_unavailable_reason"),
            agent_definitions=list(data.get("agent_definitions", [])),
        )


@dataclass(frozen=True)
class MobpackTemplatesResult:
    """Result of a mobkit/mobpacks/templates RPC call."""
    schema_version: str
    source: str
    authoring_provider: dict[str, Any]
    runtime_unavailable_reason: str | None
    blank_mobpack: dict[str, Any] | None
    sample_mobpacks: list[dict[str, Any]]
    sample_agent_definitions: list[dict[str, Any]]
    templates: dict[str, Any]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MobpackTemplatesResult:
        blank = data.get("blank_mobpack")
        return cls(
            schema_version=str(data.get("schema_version", "")),
            source=str(data.get("source", "")),
            authoring_provider=dict(data.get("authoring_provider", {})),
            runtime_unavailable_reason=data.get("runtime_unavailable_reason"),
            blank_mobpack=blank if isinstance(blank, dict) else None,
            sample_mobpacks=list(data.get("sample_mobpacks", [])),
            sample_agent_definitions=list(data.get("sample_agent_definitions", [])),
            templates=dict(data.get("templates", {})),
        )


@dataclass(frozen=True)
class MobpackCatalogsResult:
    """Result of a mobkit/mobpacks/catalogs RPC call."""
    schema_version: str
    runtime_backed: bool
    authoring_provider: dict[str, Any]
    runtime_unavailable_reason: str | None
    sources: dict[str, Any]
    templates: dict[str, Any]
    tool_catalog: list[dict[str, Any]]
    skill_realms: list[dict[str, Any]]
    blank_mobpack: dict[str, Any] | None
    sample_mobpacks: list[dict[str, Any]]
    agent_definitions: list[dict[str, Any]]
    sample_agent_definitions: list[dict[str, Any]]
    models: list[CatalogEntry]
    provider_defaults: list[ProviderDefaults]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MobpackCatalogsResult:
        blank = data.get("blank_mobpack")
        return cls(
            schema_version=str(data.get("schema_version", "")),
            runtime_backed=bool(data.get("runtime_backed", False)),
            authoring_provider=dict(data.get("authoring_provider", {})),
            runtime_unavailable_reason=data.get("runtime_unavailable_reason"),
            sources=dict(data.get("sources", {})),
            templates=dict(data.get("templates", {})),
            tool_catalog=list(data.get("tool_catalog", [])),
            skill_realms=list(data.get("skill_realms", [])),
            blank_mobpack=blank if isinstance(blank, dict) else None,
            sample_mobpacks=list(data.get("sample_mobpacks", [])),
            agent_definitions=list(data.get("agent_definitions", [])),
            sample_agent_definitions=list(data.get("sample_agent_definitions", [])),
            models=[CatalogEntry.from_dict(m) for m in data.get("models", [])],
            provider_defaults=[
                ProviderDefaults.from_dict(p) for p in data.get("provider_defaults", [])
            ],
        )


class MobMemberStatus(str, Enum):
    """Execution status for a mob member."""
    ACTIVE = "active"
    RETIRING = "retiring"
    BROKEN = "broken"
    COMPLETED = "completed"
    UNKNOWN = "unknown"


@dataclass(frozen=True)
class MobUnreachablePeer:
    """A wired peer known to be unreachable."""
    peer: str
    reason: str | None = None

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MobUnreachablePeer:
        return cls(
            peer=data.get("peer", ""),
            reason=data.get("reason"),
        )


@dataclass(frozen=True)
class PeerConnectivitySnapshot:
    """Live connectivity for a member's wired peers."""
    reachable_peer_count: int
    unknown_peer_count: int
    unreachable_peers: list[MobUnreachablePeer]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> PeerConnectivitySnapshot:
        return cls(
            reachable_peer_count=int(data.get("reachable_peer_count", 0)),
            unknown_peer_count=int(data.get("unknown_peer_count", 0)),
            unreachable_peers=[
                MobUnreachablePeer.from_dict(p)
                for p in data.get("unreachable_peers", [])
            ],
        )


@dataclass(frozen=True)
class RichMemberSnapshot:
    """Rich execution snapshot from mobkit/member_status."""
    status: str
    output_preview: str | None
    error: str | None
    tokens_used: int
    is_final: bool
    current_session_id: str | None
    peer_connectivity: PeerConnectivitySnapshot | None = None

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> RichMemberSnapshot:
        pc_raw = data.get("peer_connectivity")
        return cls(
            status=data.get("status", "unknown"),
            output_preview=data.get("output_preview"),
            error=data.get("error"),
            tokens_used=int(data.get("tokens_used", 0)),
            is_final=bool(data.get("is_final", False)),
            current_session_id=data.get("current_session_id"),
            peer_connectivity=PeerConnectivitySnapshot.from_dict(pc_raw) if pc_raw else None,
        )


@dataclass(frozen=True)
class HelperResult:
    """Result from spawn_helper or fork_helper."""
    output: str | None
    tokens_used: int
    session_id: str | None

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> HelperResult:
        return cls(
            output=data.get("output"),
            tokens_used=int(data.get("tokens_used", 0)),
            session_id=data.get("session_id"),
        )



@dataclass(frozen=True)
class MobRunSnapshot:
    """Flow run snapshot from mobkit/flow_status."""
    run_id: str
    mob_id: str
    flow_id: str
    status: str
    step_ledger: list[dict[str, Any]]
    failure_ledger: list[dict[str, Any]]

    @classmethod
    def from_dict(cls, data: dict[str, Any]) -> MobRunSnapshot:
        return cls(
            run_id=data.get("run_id", ""),
            mob_id=data.get("mob_id", ""),
            flow_id=data.get("flow_id", ""),
            status=data.get("status", "unknown"),
            step_ledger=list(data.get("step_ledger", [])),
            failure_ledger=list(data.get("failure_ledger", [])),
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
        elif category == "identity_materialization_failure":
            identity = context.get("identity", "")
            initiator = context.get("initiator", "")
            operation = context.get("operation", "")
            target = f"{identity} for {initiator}" if initiator else identity
            message = ": ".join(str(part) for part in (target, operation, error) if part)
        else:
            message = str(data)
        return cls(category=category, message=message, context=context)
