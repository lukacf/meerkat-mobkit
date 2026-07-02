"""Memory backend configuration for MobKit runtime.

The operational memory ledger (``mobkit/memory/*``) is persisted by the
gateway as local JSON under ``persistent_state``, with an optional HTTP
health-check gate. ``local_json()`` is the honest configuration for that
backend; ``elephant()`` is a deprecated alias kept for wire compatibility
with older gateways (it never wrote data to Elephant).
"""
from __future__ import annotations

import warnings
from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class LocalJsonMemoryConfig:
    """Local-JSON operational ledger backend with an optional health gate.

    When ``health_check_endpoint`` is set, the gateway health-checks
    ``GET <endpoint>/v1/health`` before every ledger read/write.
    """

    health_check_endpoint: str | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {"backend": "local_json"}
        if self.health_check_endpoint is not None:
            result["health_check_endpoint"] = self.health_check_endpoint
        return result


def local_json(health_check_endpoint: str | None = None) -> LocalJsonMemoryConfig:
    return LocalJsonMemoryConfig(health_check_endpoint=health_check_endpoint)


@dataclass(frozen=True)
class ElephantMemoryConfig:
    """Deprecated legacy config shape. See :func:`elephant`."""

    endpoint: str
    space_id: str | None = None
    collection: str | None = None
    stores: list[str] = field(default_factory=list)

    def to_dict(self) -> dict[str, Any]:
        return {
            "backend": "elephant",
            "endpoint": self.endpoint,
        }


def elephant(endpoint: str, **kwargs: Any) -> ElephantMemoryConfig:
    """Deprecated: use :func:`local_json` instead.

    Despite the name, this backend never sent data to Elephant: the gateway
    only health-checks ``endpoint`` and persists the ledger as local JSON.
    The legacy wire shape is still emitted for compatibility with older
    gateways.
    """
    warnings.warn(
        "memory.elephant() is deprecated: it only health-checks the endpoint and "
        "persists the operational ledger as local JSON; use "
        "memory.local_json(health_check_endpoint=...) instead",
        DeprecationWarning,
        stacklevel=2,
    )
    ignored = sorted(
        key for key in ("space_id", "collection", "stores") if kwargs.get(key)
    )
    if ignored:
        warnings.warn(
            f"memory.elephant() fields {', '.join(ignored)} are not sent to the "
            "gateway and have never had any effect",
            DeprecationWarning,
            stacklevel=2,
        )
    return ElephantMemoryConfig(endpoint=endpoint, **kwargs)
