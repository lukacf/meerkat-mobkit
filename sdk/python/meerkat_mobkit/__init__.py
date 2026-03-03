"""MobKit Python SDK.

All imports come from this package:
    from meerkat_mobkit import MobKit, auth, memory, session_store
    from meerkat_mobkit import DiscoverySpec, PreSpawnData, SessionQuery
    from meerkat_mobkit import SessionAgentBuilder, SessionBuildOptions
"""
from __future__ import annotations

# Builder + Runtime
from .builder import MobKit, MobKitBuilder
from .runtime import MobHandle, MobKitRuntime, SseBridge

# Data models
from .models import DiscoverySpec, PreSpawnData, SessionBuildOptions, SessionQuery

# Agent builder protocol
from .agent_builder import CallbackDispatcher, SessionAgentBuilder

# SSE bridge
from .sse import SseEvent, SseEventStream, parse_sse_stream

# Transport
from .transport import PersistentTransport, create_persistent_transport

# Config modules (importable as meerkat_mobkit.auth, etc.)
from .config import auth, memory, session_store

# Typed RPC clients (low-level)
from .client import (
    MobkitAsyncTypedClient,
    MobkitRpcError,
    MobkitTypedClient,
    create_gateway_async_transport,
    create_gateway_sync_transport,
    create_http_transport,
)

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
    "MobHandle",
    "SseBridge",
    # Data models
    "DiscoverySpec",
    "PreSpawnData",
    "SessionBuildOptions",
    "SessionQuery",
    # Agent builder
    "CallbackDispatcher",
    "SessionAgentBuilder",
    # SSE
    "SseEvent",
    "SseEventStream",
    "parse_sse_stream",
    # Transport
    "PersistentTransport",
    "create_persistent_transport",
    # Config modules
    "auth",
    "memory",
    "session_store",
    # Typed RPC clients
    "MobkitAsyncTypedClient",
    "MobkitRpcError",
    "MobkitTypedClient",
    "create_gateway_async_transport",
    "create_gateway_sync_transport",
    "create_http_transport",
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
