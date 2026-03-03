"""Memory backend configuration for MobKit runtime."""
from __future__ import annotations

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class ElephantMemoryConfig:
    """Elephant memory backend configuration."""

    endpoint: str
    space_id: str | None = None
    collection: str | None = None

    def to_dict(self) -> dict[str, Any]:
        result: dict[str, Any] = {
            "backend": "elephant",
            "endpoint": self.endpoint,
        }
        if self.space_id:
            result["space_id"] = self.space_id
        if self.collection:
            result["collection"] = self.collection
        return result


def elephant(endpoint: str, **kwargs: Any) -> ElephantMemoryConfig:
    """Create Elephant memory backend configuration."""
    return ElephantMemoryConfig(endpoint=endpoint, **kwargs)
