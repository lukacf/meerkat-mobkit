"""SSE streaming bridge for ASGI integration.

Bridges SSE events from the Rust runtime to Python async generators,
suitable for use with FastAPI's ``StreamingResponse`` or Starlette's
``EventSourceResponse``.
"""

from __future__ import annotations

import asyncio
import json
from typing import Any, AsyncIterator


class SseEvent:
    """A single SSE event."""

    __slots__ = ("id", "event", "data")

    def __init__(
        self,
        *,
        id: str | None = None,
        event: str = "message",
        data: str = "",
    ) -> None:
        self.id = id
        self.event = event
        self.data = data

    def encode(self) -> str:
        """Encode as SSE wire format."""
        lines: list[str] = []
        if self.id is not None:
            lines.append(f"id: {self.id}")
        if self.event != "message":
            lines.append(f"event: {self.event}")
        for line in self.data.split("\n"):
            lines.append(f"data: {line}")
        lines.append("")
        lines.append("")
        return "\n".join(lines)

    def to_dict(self) -> dict[str, Any]:
        """Return a plain-dict representation."""
        result: dict[str, Any] = {"event": self.event, "data": self.data}
        if self.id is not None:
            result["id"] = self.id
        return result


class SseEventStream:
    """Async iterator that yields SSE-formatted event strings.

    Use with FastAPI / Starlette ``StreamingResponse``::

        stream = SseEventStream(source)
        return StreamingResponse(stream, media_type="text/event-stream")
    """

    def __init__(
        self,
        source: AsyncIterator[dict[str, Any]],
        *,
        keep_alive_interval: float = 15.0,
    ) -> None:
        self._source = source
        self._keep_alive_interval = keep_alive_interval

    async def __aiter__(self) -> AsyncIterator[str]:
        """Yield SSE-formatted strings."""
        async for event_data in self._with_keep_alive():
            if event_data is None:
                yield ": keep-alive\n\n"
            else:
                raw_data = event_data.get("data", "")
                if not isinstance(raw_data, str):
                    raw_data = json.dumps(raw_data)
                event = SseEvent(
                    id=event_data.get("id"),
                    event=event_data.get("event", "message"),
                    data=raw_data,
                )
                yield event.encode()

    async def _with_keep_alive(self) -> AsyncIterator[dict[str, Any] | None]:
        """Wrap *source* with periodic keep-alive signals."""
        source_iter = self._source.__aiter__()
        while True:
            try:
                event = await asyncio.wait_for(
                    source_iter.__anext__(),
                    timeout=self._keep_alive_interval,
                )
                yield event
            except asyncio.TimeoutError:
                yield None
            except StopAsyncIteration:
                break


async def parse_sse_stream(
    response_body: AsyncIterator[bytes],
) -> AsyncIterator[SseEvent]:
    """Parse an SSE byte stream into :class:`SseEvent` objects.

    Useful for consuming SSE from the Rust runtime's HTTP endpoints.
    """
    buffer = ""
    current_id: str | None = None
    current_event = "message"
    current_data: list[str] = []

    async for chunk in response_body:
        buffer += chunk.decode("utf-8", errors="replace")
        while "\n" in buffer:
            line, buffer = buffer.split("\n", 1)
            line = line.rstrip("\r")

            if not line:
                # Empty line marks the end of an event block.
                if current_data:
                    yield SseEvent(
                        id=current_id,
                        event=current_event,
                        data="\n".join(current_data),
                    )
                current_id = None
                current_event = "message"
                current_data = []
            elif line.startswith(":"):
                continue  # comment (e.g. keep-alive)
            elif line.startswith("id: "):
                current_id = line[4:]
            elif line.startswith("event: "):
                current_event = line[7:]
            elif line.startswith("data: "):
                current_data.append(line[6:])
            elif line.startswith("data:"):
                current_data.append(line[5:])
