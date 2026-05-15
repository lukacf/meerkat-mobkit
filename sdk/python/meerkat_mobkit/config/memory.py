"""Memory backend configuration for MobKit runtime."""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any


@dataclass(frozen=True)
class ElephantMemoryConfig:
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
    return ElephantMemoryConfig(endpoint=endpoint, **kwargs)
