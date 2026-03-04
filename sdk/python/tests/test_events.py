"""Tests for typed event construction (from_sse)."""
import json

from meerkat_mobkit._sse import SseEvent
from meerkat_mobkit.events import AgentEvent, InteractionEvent, MobEvent


class TestMobEvent:
    def test_from_sse(self):
        payload = json.dumps({"source": "runtime", "timestamp_ms": 1000, "extra": 1})
        sse = SseEvent(id="ev-1", event="mob_event", data=payload)
        ev = MobEvent.from_sse(sse)
        assert ev.event_id == "ev-1"
        assert ev.source == "runtime"
        assert ev.timestamp_ms == 1000
        assert ev.data["extra"] == 1


class TestAgentEvent:
    def test_from_sse(self):
        payload = json.dumps({"action": "speak"})
        sse = SseEvent(id="ev-2", event="agent_event", data=payload)
        ev = AgentEvent.from_sse(sse, agent_id="agent-1")
        assert ev.event_id == "ev-2"
        assert ev.agent_id == "agent-1"
        assert ev.event_type == "agent_event"
        assert ev.data == {"action": "speak"}


class TestInteractionEvent:
    def test_from_sse(self):
        sse = SseEvent(id="ev-3", event="interaction", data="hello")
        ev = InteractionEvent.from_sse(sse)
        assert ev.event_id == "ev-3"
        assert ev.event_type == "interaction"
        assert ev.data == "hello"
