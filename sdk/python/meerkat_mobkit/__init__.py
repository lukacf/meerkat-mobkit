"""MobKit Python SDK.

Usage::

    from meerkat_mobkit import MobKit, MobKitRuntime, MobKitBuilder
    from meerkat_mobkit import DiscoverySpec, PreSpawnData, SessionQuery
    from meerkat_mobkit import SessionAgentBuilder, SessionBuildOptions
    from meerkat_mobkit.errors import MobKitError, RpcError, NotConnectedError
    from meerkat_mobkit.types import StatusResult, CapabilitiesResult
    from meerkat_mobkit.events import MobEvent, AgentEvent

Module authoring helpers are available via::

    from meerkat_mobkit.helpers import ModuleSpec, define_module, ...
"""
from __future__ import annotations

# Builder + Runtime
from .builder import MobKit, MobKitBuilder
from .runtime import MobKitRuntime, ToolCaller

# Data models
from .models import DiscoverySpec, PreSpawnData, SessionBuildOptions, SessionQuery

# Agent builder protocol (public contract — CallbackDispatcher is internal)
from .agent_builder import SessionAgentBuilder

# Errors
from .errors import (
    CapabilityUnavailableError,
    ContractMismatchError,
    MobKitError,
    NotConnectedError,
    RpcError,
    TransportError,
)

# Typed return models
from .types import (
    CallToolResult,
    CapabilitiesResult,
    DeliveryHistoryResult,
    DeliveryResult,
    ErrorCategory,
    ErrorEvent,
    EventEnvelope,
    EventQuery,
    GatingAuditEntry,
    GatingDecisionResult,
    GatingEvaluateResult,
    GatingPendingEntry,
    KeepAliveConfig,
    MEMBER_STATE_ACTIVE,
    MEMBER_STATE_RETIRING,
    MemberSnapshot,
    MemoryIndexResult,
    MemoryQueryResult,
    MemoryStoreInfo,
    PersistedEvent,
    ReconcileEdgesReport,
    ReconcileResult,
    RediscoverReport,
    RoutingResolution,
    RuntimeRouteResult,
    SpawnMemberResult,
    SpawnResult,
    StatusResult,
    SubscribeResult,
    UnifiedAgentEvent,
    UnifiedModuleEvent,
)

# Typed events
from .events import (
    AgentEvent,
    Event,
    EventStream,
    MobEvent,
    RunCompleted,
    RunFailed,
    RunStarted,
    TextComplete,
    TextDelta,
    ToolCallRequested,
    ToolExecutionCompleted,
    ToolExecutionStarted,
    ToolResultReceived,
    TurnCompleted,
    TurnStarted,
    UnknownEvent,
)

# Config modules (importable as meerkat_mobkit.auth, etc.)
from .config import auth, memory, session_store

__all__ = [
    # Builder + Runtime
    "MobKit",
    "MobKitBuilder",
    "MobKitRuntime",
    # Data models
    "DiscoverySpec",
    "PreSpawnData",
    "SessionBuildOptions",
    "SessionQuery",
    # Agent builder
    "SessionAgentBuilder",
    # Errors
    "MobKitError",
    "TransportError",
    "RpcError",
    "CapabilityUnavailableError",
    "ContractMismatchError",
    "NotConnectedError",
    # Typed return models
    "StatusResult",
    "CapabilitiesResult",
    "ReconcileResult",
    "SpawnResult",
    "SpawnMemberResult",
    "SubscribeResult",
    "KeepAliveConfig",
    "EventEnvelope",
    "RoutingResolution",
    "DeliveryResult",
    "DeliveryHistoryResult",
    "MemoryQueryResult",
    "MemoryStoreInfo",
    "MemoryIndexResult",
    "MEMBER_STATE_ACTIVE",
    "MEMBER_STATE_RETIRING",
    "MemberSnapshot",
    "RuntimeRouteResult",
    "GatingEvaluateResult",
    "GatingDecisionResult",
    "GatingAuditEntry",
    "GatingPendingEntry",
    "CallToolResult",
    "ErrorCategory",
    "ErrorEvent",
    "EventQuery",
    "PersistedEvent",
    "UnifiedAgentEvent",
    "UnifiedModuleEvent",
    "ReconcileEdgesReport",
    "RediscoverReport",
    "ToolCaller",
    # Typed events
    "Event",
    "MobEvent",
    "AgentEvent",
    "EventStream",
    "RunStarted",
    "RunCompleted",
    "RunFailed",
    "TurnStarted",
    "TextDelta",
    "TextComplete",
    "ToolCallRequested",
    "ToolResultReceived",
    "TurnCompleted",
    "ToolExecutionStarted",
    "ToolExecutionCompleted",
    "UnknownEvent",
    # Config modules
    "auth",
    "memory",
    "session_store",
]
