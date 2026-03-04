"""Tests for typed return models (from_dict)."""
from meerkat_mobkit.types import (
    CapabilitiesResult,
    DeliveryResult,
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
    def test_from_dict(self):
        r = SpawnResult.from_dict({"accepted": True, "module_id": "mod-1"})
        assert r.accepted is True
        assert r.module_id == "mod-1"


class TestSpawnMemberResult:
    def test_from_dict(self):
        r = SpawnMemberResult.from_dict({"accepted": False, "module_id": "mod-2"})
        assert r.accepted is False
        assert r.module_id == "mod-2"


class TestSubscribeResult:
    def test_from_dict(self):
        r = SubscribeResult.from_dict(
            {
                "scope": "mob",
                "replay_from_event_id": "ev-1",
                "keep_alive": {"interval": 15},
                "keep_alive_comment": "ping",
                "event_frames": ["frame1"],
                "events": [{"type": "init"}],
            }
        )
        assert r.scope == "mob"
        assert r.replay_from_event_id == "ev-1"
        assert r.keep_alive == {"interval": 15}
        assert r.keep_alive_comment == "ping"
        assert r.event_frames == ["frame1"]
        assert r.events == [{"type": "init"}]


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
