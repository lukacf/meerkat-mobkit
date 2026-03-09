"""Typed event models for MobKit SDK streaming."""
from __future__ import annotations

import json
from dataclasses import dataclass
from typing import Any, AsyncIterator

from ._sse import SseEvent


@dataclass(frozen=True)
class MobEvent:
    """A mob-level event from the runtime."""
    event_id: str
    source: str
    timestamp_ms: int
    data: Any

    @classmethod
    def from_sse(cls, event: SseEvent) -> MobEvent:
        try:
            parsed = json.loads(event.data)
        except (json.JSONDecodeError, TypeError):
            parsed = event.data
        return cls(
            event_id=event.id or "",
            source=parsed.get("source", "") if isinstance(parsed, dict) else "",
            timestamp_ms=parsed.get("timestamp_ms", 0) if isinstance(parsed, dict) else 0,
            data=parsed,
        )


@dataclass(frozen=True)
class AgentEvent:
    """A per-agent event from the runtime."""
    event_id: str
    agent_id: str
    event_type: str
    data: Any

    @classmethod
    def from_sse(cls, event: SseEvent, agent_id: str = "") -> AgentEvent:
        try:
            parsed = json.loads(event.data)
        except (json.JSONDecodeError, TypeError):
            parsed = event.data
        return cls(
            event_id=event.id or "",
            agent_id=agent_id,
            event_type=event.event,
            data=parsed,
        )


class EventStream:
    """Typed async iterator wrapping raw SSE events into domain events."""

    def __init__(self, source: AsyncIterator[SseEvent], event_cls: type, **kwargs: Any):
        self._source = source
        self._event_cls = event_cls
        self._kwargs = kwargs

    def __aiter__(self) -> EventStream:
        return self

    async def __anext__(self) -> MobEvent | AgentEvent:
        event = await self._source.__anext__()
        return self._event_cls.from_sse(event, **self._kwargs)
