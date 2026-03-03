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
        result: dict[str, Any] = {
            "backend": "elephant",
            "endpoint": self.endpoint,
        }
        if self.space_id:
            result["space_id"] = self.space_id
        if self.collection:
            result["collection"] = self.collection
        if self.stores:
            result["stores"] = self.stores
        return result


def elephant(endpoint: str, **kwargs: Any) -> ElephantMemoryConfig:
    return ElephantMemoryConfig(endpoint=endpoint, **kwargs)
