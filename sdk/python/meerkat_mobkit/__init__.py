"""MobKit Python SDK.

Usage::

    from meerkat_mobkit import MobKit, MobKitRuntime, MobKitBuilder
    from meerkat_mobkit import DiscoverySpec, PreSpawnData, SessionQuery
    from meerkat_mobkit import SessionAgentBuilder, SessionBuildOptions
    from meerkat_mobkit.errors import MobKitError, RpcError, NotConnectedError
    from meerkat_mobkit.types import StatusResult, CapabilitiesResult
    from meerkat_mobkit.events import MobEvent, AgentEvent, InteractionEvent
"""
from __future__ import annotations

# Builder + Runtime
from .builder import MobKit, MobKitBuilder
from .runtime import MobKitRuntime

# Data models
from .models import DiscoverySpec, PreSpawnData, SessionBuildOptions, SessionQuery

# Agent builder protocol
from .agent_builder import CallbackDispatcher, SessionAgentBuilder

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
    CapabilitiesResult,
    DeliveryResult,
    MemoryQueryResult,
    ReconcileResult,
    RoutingResolution,
    SpawnMemberResult,
    SpawnResult,
    StatusResult,
    SubscribeResult,
)

# Typed events
from .events import AgentEvent, EventStream, InteractionEvent, MobEvent

# Config modules (importable as meerkat_mobkit.auth, etc.)
from .config import auth, memory, session_store

# Module authoring helpers
from .helpers import (
    ModuleDefinition,
    ModuleSpec,
    ModuleTool,
    build_console_experience_route,
    build_console_modules_route,
    build_console_route,
    build_console_routes,
    build_module_spec,
    decorate_module_spec,
    decorate_module_tool,
    define_module,
    define_module_spec,
    define_module_tool,
)

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
    "CallbackDispatcher",
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
    "RoutingResolution",
    "DeliveryResult",
    "MemoryQueryResult",
    # Typed events
    "MobEvent",
    "AgentEvent",
    "InteractionEvent",
    "EventStream",
    # Config modules
    "auth",
    "memory",
    "session_store",
    # Module authoring
    "ModuleDefinition",
    "ModuleSpec",
    "ModuleTool",
    "build_console_experience_route",
    "build_console_modules_route",
    "build_console_route",
    "build_console_routes",
    "build_module_spec",
    "decorate_module_spec",
    "decorate_module_tool",
    "define_module",
    "define_module_spec",
    "define_module_tool",
]
