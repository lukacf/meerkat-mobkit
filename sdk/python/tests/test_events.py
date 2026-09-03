"""Tests for typed event construction and parsing."""
import json

from meerkat_mobkit._sse import SseEvent
from meerkat_mobkit.events import (
    AgentEvent,
    Event,
    MobEvent,
    RunCompleted,
    RunFailed,
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

    def test_run_failed_derives_error_from_meerkat_error_report(self):
        # Wire shape of meerkat-core 0.8.32 `AgentEvent::RunFailed`: the typed
        # `error_report` is the only failure truth; there is no flat `error`.
        raw = {
            "type": "run_failed",
            "session_id": "01a068a0-f911-7331-8187-d436145ac895",
            "error_report": {
                "class": "llm",
                "reason": {"reason_type": "llm_auth_error"},
                "message": "LLM error: authentication failed (401)",
            },
        }
        ev = parse_agent_event(raw)
        assert isinstance(ev, RunFailed)
        assert ev.error == "LLM error: authentication failed (401)"
        assert ev.error_report == raw["error_report"]
        assert ev.error_class == "llm"
        assert ev.reason_type == "llm_auth_error"

    def test_run_failed_keeps_a_flat_error_when_the_wire_carries_one(self):
        ev = parse_agent_event({"type": "run_failed", "session_id": "s1", "error": "boom"})
        assert isinstance(ev, RunFailed)
        assert ev.error == "boom"
        assert ev.error_report == {}
        assert ev.error_class == ""
        assert ev.reason_type == ""

    def test_run_failed_without_typed_reason_has_empty_reason_type(self):
        ev = parse_agent_event({
            "type": "run_failed",
            "session_id": "s1",
            "error_report": {"class": "internal", "message": "runner gave up"},
        })
        assert isinstance(ev, RunFailed)
        assert ev.error == "runner gave up"
        assert ev.error_class == "internal"
        assert ev.reason_type == ""

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

    def test_match_run_failed_error_sees_the_derived_message(self):
        ev = parse_agent_event({
            "type": "run_failed",
            "session_id": "s1",
            "error_report": {"class": "llm", "message": "rate limited"},
        })
        match ev:
            case RunFailed(error=err):
                assert err == "rate limited"
            case _:
                raise AssertionError("should match RunFailed")


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
