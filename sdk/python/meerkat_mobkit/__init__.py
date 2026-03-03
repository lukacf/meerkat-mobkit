"""MobKit Python SDK — high-level runtime surface."""

from __future__ import annotations

# Re-export low-level clients for backward compat
from meerkat_mobkit_sdk import (
    MobkitAsyncTypedClient,
    MobkitRpcError,
    MobkitTypedClient,
    create_gateway_async_transport,
    create_gateway_sync_transport,
    create_http_transport,
)

from .transport import PersistentTransport, create_persistent_transport

__all__ = [
    # Low-level (MK-029)
    "MobkitAsyncTypedClient",
    "MobkitRpcError",
    "MobkitTypedClient",
    "create_gateway_async_transport",
    "create_gateway_sync_transport",
    "create_http_transport",
    # Transport
    "PersistentTransport",
    "create_persistent_transport",
]
