"""Tests for typed event construction and parsing."""
import json

from meerkat_mobkit._sse import SseEvent
from meerkat_mobkit.events import (
    AgentEvent,
    Event,
    MobEvent,
    RunCompleted,
    RunStarted,
    TextDelta,
    ToolCallRequested,
    UnknownEvent,
    parse_agent_event,
)


class TestParseAgentEvent:
    def test_text_delta(self):
        ev = parse_agent_event({"type": "text_delta", "delta": "hello"})
        assert isinstance(ev, TextDelta)
        assert ev.delta == "hello"

    def test_run_started(self):
        ev = parse_agent_event({"type": "run_started", "session_id": "s1", "prompt": "hi"})
        assert isinstance(ev, RunStarted)
        assert ev.session_id == "s1"
        assert ev.prompt == "hi"

    def test_run_completed(self):
        ev = parse_agent_event({"type": "run_completed", "session_id": "s1", "result": "done"})
        assert isinstance(ev, RunCompleted)
        assert ev.result == "done"

    def test_tool_call_requested(self):
        ev = parse_agent_event({
            "type": "tool_call_requested",
            "id": "tc-1",
            "name": "search",
            "args": {"query": "test"},
        })
        assert isinstance(ev, ToolCallRequested)
        assert ev.name == "search"
        assert ev.args == {"query": "test"}

    def test_unknown_event_type(self):
        ev = parse_agent_event({"type": "future_event", "foo": "bar"})
        assert isinstance(ev, UnknownEvent)
        assert ev.type == "future_event"
        assert ev.data == {"type": "future_event", "foo": "bar"}

    def test_all_events_are_event_subclass(self):
        ev = parse_agent_event({"type": "text_delta", "delta": "x"})
        assert isinstance(ev, Event)
        ev2 = parse_agent_event({"type": "unknown_thing"})
        assert isinstance(ev2, Event)


class TestPatternMatching:
    def test_match_text_delta(self):
        ev = parse_agent_event({"type": "text_delta", "delta": "chunk"})
        match ev:
            case TextDelta(delta=d):
                assert d == "chunk"
            case _:
                raise AssertionError("should match TextDelta")

    def test_match_run_completed(self):
        ev = parse_agent_event({"type": "run_completed", "result": "done"})
        match ev:
            case RunCompleted(result=r):
                assert r == "done"
            case _:
                raise AssertionError("should match RunCompleted")


class TestAgentEvent:
    def test_from_sse_typed(self):
        payload = json.dumps({"type": "text_delta", "delta": "hello"})
        sse = SseEvent(id="ev-1", event="text_delta", data=payload)
        ev = AgentEvent.from_sse(sse, agent_id="agent-1")
        assert ev.event_type == "text_delta"
        assert isinstance(ev.event, TextDelta)
        assert ev.event.delta == "hello"

    def test_from_sse_unknown(self):
        payload = json.dumps({"type": "future_thing", "x": 1})
        sse = SseEvent(id="ev-2", event="future_thing", data=payload)
        ev = AgentEvent.from_sse(sse)
        assert isinstance(ev.event, UnknownEvent)


class TestMobEvent:
    def test_from_sse_typed(self):
        payload = json.dumps({
            "member_id": "agent-1",
            "timestamp_ms": 1000,
            "payload": {"type": "text_delta", "delta": "hi"},
        })
        sse = SseEvent(id="ev-1", event="mob_event", data=payload)
        ev = MobEvent.from_sse(sse)
        assert ev.member_id == "agent-1"
        assert ev.timestamp_ms == 1000
        assert isinstance(ev.event, TextDelta)
        assert ev.event.delta == "hi"

    def test_from_sse_unknown_payload(self):
        payload = json.dumps({
            "member_id": "agent-2",
            "payload": {"type": "new_thing"},
        })
        sse = SseEvent(id="ev-2", event="mob_event", data=payload)
        ev = MobEvent.from_sse(sse)
        assert ev.member_id == "agent-2"
        assert isinstance(ev.event, UnknownEvent)
