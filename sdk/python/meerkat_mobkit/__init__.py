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
    DeliveryResult,
    EventEnvelope,
    KeepAliveConfig,
    MemoryQueryResult,
    ReconcileResult,
    RoutingResolution,
    SpawnMemberResult,
    SpawnResult,
    StatusResult,
    SubscribeResult,
)

# Typed events
from .events import AgentEvent, EventStream, MobEvent

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
    "MemoryQueryResult",
    "CallToolResult",
    "ToolCaller",
    # Typed events
    "MobEvent",
    "AgentEvent",
    "EventStream",
    # Config modules
    "auth",
    "memory",
    "session_store",
]
