"""Tests for typed return models (from_dict)."""
from meerkat_mobkit.types import (
    CallToolResult,
    CapabilitiesResult,
    DeliveryResult,
    EventEnvelope,
    KeepAliveConfig,
    MemoryQueryResult,
    ReconcileResult,
    RoutingResolution,
    SpawnMemberResult,
    SpawnResult,
    StatusResult,
    SubscribeResult,
)


class TestStatusResult:
    def test_from_dict(self):
        r = StatusResult.from_dict(
            {"contract_version": "1.0", "running": True, "loaded_modules": ["a", "b"]}
        )
        assert r.contract_version == "1.0"
        assert r.running is True
        assert r.loaded_modules == ["a", "b"]


class TestCapabilitiesResult:
    def test_from_dict(self):
        r = CapabilitiesResult.from_dict(
            {"contract_version": "2.0", "methods": ["status"], "loaded_modules": ["x"]}
        )
        assert r.contract_version == "2.0"
        assert r.methods == ["status"]
        assert r.loaded_modules == ["x"]


class TestReconcileResult:
    def test_from_dict(self):
        r = ReconcileResult.from_dict(
            {"accepted": True, "reconciled_modules": ["m1"], "added": 1}
        )
        assert r.accepted is True
        assert r.reconciled_modules == ["m1"]
        assert r.added == 1


class TestSpawnResult:
    def test_from_dict_module_spawn(self):
        r = SpawnResult.from_dict({"accepted": True, "module_id": "mod-1"})
        assert r.accepted is True
        assert r.module_id == "mod-1"
        assert r.meerkat_id is None
        assert r.profile is None

    def test_from_dict_discovery_spawn(self):
        r = SpawnResult.from_dict({
            "accepted": True,
            "module_id": "mod-1",
            "meerkat_id": "mk-123",
            "profile": "assistant",
        })
        assert r.meerkat_id == "mk-123"
        assert r.profile == "assistant"

    def test_from_dict_no_module_id(self):
        """Rust discovery-path may not return module_id."""
        r = SpawnResult.from_dict({"accepted": True, "meerkat_id": "mk-123"})
        assert r.accepted is True
        assert r.module_id == ""
        assert r.meerkat_id == "mk-123"


class TestSpawnMemberResult:
    def test_is_spawn_result_alias(self):
        assert SpawnMemberResult is SpawnResult


class TestKeepAliveConfig:
    def test_from_dict(self):
        r = KeepAliveConfig.from_dict({"interval_ms": 15000, "event": "ping"})
        assert r.interval_ms == 15000
        assert r.event == "ping"


class TestEventEnvelope:
    def test_from_dict(self):
        r = EventEnvelope.from_dict({
            "event_id": "ev-1",
            "source": "agent-1",
            "timestamp_ms": 1234567890,
            "event": {"kind": "ready"},
        })
        assert r.event_id == "ev-1"
        assert r.source == "agent-1"
        assert r.timestamp_ms == 1234567890
        assert r.event == {"kind": "ready"}


class TestSubscribeResult:
    def test_from_dict(self):
        r = SubscribeResult.from_dict(
            {
                "scope": "mob",
                "replay_from_event_id": "ev-1",
                "keep_alive": {"interval_ms": 15000, "event": "ping"},
                "keep_alive_comment": "ping",
                "event_frames": ["frame1"],
                "events": [
                    {
                        "event_id": "ev-2",
                        "source": "agent-1",
                        "timestamp_ms": 100,
                        "event": {"kind": "init"},
                    }
                ],
            }
        )
        assert r.scope == "mob"
        assert r.replay_from_event_id == "ev-1"
        assert isinstance(r.keep_alive, KeepAliveConfig)
        assert r.keep_alive.interval_ms == 15000
        assert r.keep_alive.event == "ping"
        assert r.keep_alive_comment == "ping"
        assert r.event_frames == ["frame1"]
        assert len(r.events) == 1
        assert isinstance(r.events[0], EventEnvelope)
        assert r.events[0].event_id == "ev-2"
        assert r.events[0].event == {"kind": "init"}


class TestRoutingResolution:
    def test_from_dict(self):
        r = RoutingResolution.from_dict(
            {"recipient": "agent-1", "route": {"path": "/a"}}
        )
        assert r.recipient == "agent-1"
        assert r.route == {"path": "/a"}


class TestDeliveryResult:
    def test_from_dict(self):
        r = DeliveryResult.from_dict({"delivered": True, "delivery_id": "d-1"})
        assert r.delivered is True
        assert r.delivery_id == "d-1"


class TestMemoryQueryResult:
    def test_from_dict(self):
        r = MemoryQueryResult.from_dict({"results": [{"key": "val"}]})
        assert r.results == [{"key": "val"}]


class TestCallToolResult:
    def test_from_dict(self):
        r = CallToolResult.from_dict({
            "module_id": "gmail",
            "tool": "gmail_search",
            "result": {"messages": [{"id": "1", "subject": "Hello"}]},
        })
        assert r.module_id == "gmail"
        assert r.tool == "gmail_search"
        assert r.result == {"messages": [{"id": "1", "subject": "Hello"}]}
