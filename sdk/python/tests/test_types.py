"""Tests for typed return models (from_dict)."""
import pytest
from meerkat_mobkit.types import (
    CallToolResult,
    CapabilitiesResult,
    DeliveryHistoryResult,
    DeliveryResult,
    ErrorEvent,
    EventEnvelope,
    EventQuery,
    GatingAuditEntry,
    GatingDecisionResult,
    GatingEvaluateResult,
    GatingPendingEntry,
    KeepAliveConfig,
    MEMBER_STATE_ACTIVE,
    MEMBER_STATE_RETIRING,
    MemberSnapshot,
    MemoryIndexResult,
    MemoryQueryResult,
    MemoryStoreInfo,
    PersistedEvent,
    ReconcileEdgesReport,
    ReconcileResult,
    RediscoverReport,
    RoutingResolution,
    RuntimeRouteResult,
    SendMessageResult,
    SpawnMemberResult,
    SpawnResult,
    StatusResult,
    SubscribeResult,
    UnifiedAgentEvent,
    UnifiedModuleEvent,
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
        assert r.runtime_capabilities is None

    def test_from_dict_with_runtime_capabilities(self):
        r = CapabilitiesResult.from_dict(
            {
                "contract_version": "0.2.0",
                "methods": ["status"],
                "loaded_modules": ["x"],
                "runtime_capabilities": {
                    "can_spawn_members": True,
                    "can_send_messages": True,
                    "can_wire_members": False,
                    "can_retire_members": True,
                    "available_spawn_modes": ["module", "profile"],
                },
            }
        )
        assert r.runtime_capabilities is not None
        assert r.runtime_capabilities.can_spawn_members is True
        assert r.runtime_capabilities.can_wire_members is False
        assert r.runtime_capabilities.available_spawn_modes == ["module", "profile"]


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


class TestSendMessageResult:
    def test_from_dict(self):
        r = SendMessageResult.from_dict(
            {
                "accepted": True,
                "member_id": "lead-1",
                "session_id": "s-1",
            }
        )
        assert r.accepted is True
        assert r.member_id == "lead-1"
        assert r.session_id == "s-1"


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


class TestToolCaller:
    @pytest.mark.asyncio
    async def test_call_unwraps_result(self):
        """ToolCaller.__call__ should unwrap CallToolResult.result."""
        from unittest.mock import AsyncMock
        from meerkat_mobkit.runtime import ToolCaller

        mock_handle = AsyncMock()
        mock_handle.call_tool.return_value = CallToolResult.from_dict({
            "module_id": "google-workspace",
            "tool": "gmail_search",
            "result": [{"id": "1", "subject": "Hello"}],
        })

        gmail = ToolCaller(mock_handle, "google-workspace")
        messages = await gmail("gmail_search", query="is:unread")

        assert messages == [{"id": "1", "subject": "Hello"}]
        mock_handle.call_tool.assert_called_once_with(
            "google-workspace", "gmail_search", {"query": "is:unread"}
        )

    def test_tool_caller_stores_module_id(self):
        from unittest.mock import AsyncMock
        from meerkat_mobkit.runtime import ToolCaller, MobHandle
        mock_handle = AsyncMock(spec=MobHandle)
        caller = ToolCaller(mock_handle, "my-module")
        assert caller._module_id == "my-module"
        assert caller._mob_handle is mock_handle

    @pytest.mark.asyncio
    async def test_call_propagates_errors(self):
        from unittest.mock import AsyncMock
        from meerkat_mobkit.runtime import ToolCaller

        mock_handle = AsyncMock()
        mock_handle.call_tool.side_effect = RuntimeError("module not loaded")
        gmail = ToolCaller(mock_handle, "google-workspace")
        with pytest.raises(RuntimeError, match="module not loaded"):
            await gmail("gmail_search", query="test")


class TestMemberSnapshot:
    def test_from_dict(self):
        r = MemberSnapshot.from_dict(
            {
                "meerkat_id": "agent-1",
                "profile": "worker",
                "state": "active",
                "wired_to": ["agent-2"],
                "labels": {"role": "lead"},
            }
        )
        assert r.meerkat_id == "agent-1"
        assert r.profile == "worker"
        assert r.state == "active"
        assert r.wired_to == ["agent-2"]
        assert r.labels == {"role": "lead"}


class TestRuntimeRouteResult:
    def test_from_dict(self):
        r = RuntimeRouteResult.from_dict(
            {
                "route_key": "r1",
                "recipient": "user-1",
                "channel": "slack",
                "sink": "notify",
                "target_module": "comms",
            }
        )
        assert r.route_key == "r1"
        assert r.recipient == "user-1"
        assert r.channel == "slack"
        assert r.sink == "notify"
        assert r.target_module == "comms"


class TestDeliveryHistoryResult:
    def test_from_dict(self):
        r = DeliveryHistoryResult.from_dict(
            {"deliveries": [{"delivery_id": "d1", "status": "sent"}]}
        )
        assert r.deliveries == [{"delivery_id": "d1", "status": "sent"}]


class TestGatingEvaluateResult:
    def test_from_dict(self):
        r = GatingEvaluateResult.from_dict(
            {
                "action_id": "a1",
                "action": "send_email",
                "actor_id": "bot-1",
                "risk_tier": "r1",
                "outcome": "allowed",
                "pending_id": None,
            }
        )
        assert r.action_id == "a1"
        assert r.action == "send_email"
        assert r.actor_id == "bot-1"
        assert r.risk_tier == "r1"
        assert r.outcome == "allowed"
        assert r.pending_id is None


class TestGatingDecisionResult:
    def test_from_dict(self):
        r = GatingDecisionResult.from_dict(
            {"pending_id": "p1", "action_id": "a1", "decision": "approve"}
        )
        assert r.pending_id == "p1"
        assert r.action_id == "a1"
        assert r.decision == "approve"


class TestGatingAuditEntry:
    def test_from_dict(self):
        r = GatingAuditEntry.from_dict(
            {
                "audit_id": "au1",
                "timestamp_ms": 1000,
                "event_type": "evaluate",
                "action_id": "a1",
                "actor_id": "bot-1",
                "risk_tier": "r0",
                "outcome": "allowed",
            }
        )
        assert r.audit_id == "au1"
        assert r.timestamp_ms == 1000
        assert r.event_type == "evaluate"
        assert r.action_id == "a1"
        assert r.actor_id == "bot-1"
        assert r.risk_tier == "r0"
        assert r.outcome == "allowed"


class TestGatingPendingEntry:
    def test_from_dict(self):
        r = GatingPendingEntry.from_dict(
            {
                "pending_id": "p1",
                "action_id": "a1",
                "action": "deploy",
                "actor_id": "bot-1",
                "risk_tier": "r2",
                "created_at_ms": 5000,
            }
        )
        assert r.pending_id == "p1"
        assert r.action_id == "a1"
        assert r.action == "deploy"
        assert r.actor_id == "bot-1"
        assert r.risk_tier == "r2"
        assert r.created_at_ms == 5000


class TestMemoryStoreInfo:
    def test_from_dict(self):
        r = MemoryStoreInfo.from_dict(
            {"store": "knowledge_graph", "record_count": 42}
        )
        assert r.store == "knowledge_graph"
        assert r.record_count == 42


class TestMemoryIndexResult:
    def test_from_dict(self):
        r = MemoryIndexResult.from_dict(
            {
                "entity": "user-1",
                "topic": "prefs",
                "store": "knowledge_graph",
                "assertion_id": "mem-001",
            }
        )
        assert r.entity == "user-1"
        assert r.topic == "prefs"
        assert r.store == "knowledge_graph"
        assert r.assertion_id == "mem-001"


class TestReconcileEdgesReport:
    def test_from_dict(self):
        r = ReconcileEdgesReport.from_dict(
            {
                "desired_edges": [],
                "wired_edges": [],
                "unwired_edges": [],
                "retained_edges": [],
                "preexisting_edges": [],
                "skipped_missing_members": [],
                "pruned_stale_managed_edges": [],
                "failures": [],
            }
        )
        assert r.desired_edges == []
        assert r.wired_edges == []
        assert r.unwired_edges == []
        assert r.retained_edges == []
        assert r.preexisting_edges == []
        assert r.skipped_missing_members == []
        assert r.pruned_stale_managed_edges == []
        assert r.failures == []


class TestRediscoverReport:
    def test_from_dict(self):
        r = RediscoverReport.from_dict(
            {
                "spawned": ["a1"],
                "edges": {
                    "desired_edges": [],
                    "wired_edges": [],
                    "unwired_edges": [],
                    "retained_edges": [],
                    "preexisting_edges": [],
                    "skipped_missing_members": [],
                    "pruned_stale_managed_edges": [],
                    "failures": [],
                },
            }
        )
        assert r.spawned == ["a1"]
        assert isinstance(r.edges, ReconcileEdgesReport)
        assert r.edges.desired_edges == []
        assert r.edges.wired_edges == []
        assert r.edges.unwired_edges == []
        assert r.edges.retained_edges == []
        assert r.edges.preexisting_edges == []
        assert r.edges.skipped_missing_members == []
        assert r.edges.pruned_stale_managed_edges == []
        assert r.edges.failures == []


class TestPersistedEvent:
    def test_from_dict(self):
        r = PersistedEvent.from_dict(
            {
                "id": "ev1",
                "seq": 1,
                "timestamp_ms": 1000,
                "member_id": "agent-1",
                "event": {"Agent": {"agent_id": "agent-1", "event_type": "run_completed"}},
            }
        )
        assert r.id == "ev1"
        assert r.seq == 1
        assert r.timestamp_ms == 1000
        assert r.member_id == "agent-1"
        assert isinstance(r.event, UnifiedAgentEvent)
        assert r.event.agent_id == "agent-1"
        assert r.event.event_type == "run_completed"


class TestUnifiedAgentEvent:
    def test_direct_construction(self):
        e = UnifiedAgentEvent(agent_id="agent-1", event_type="run_completed")
        assert e.agent_id == "agent-1"
        assert e.event_type == "run_completed"


class TestUnifiedModuleEvent:
    def test_direct_construction(self):
        e = UnifiedModuleEvent(module="router", event_type="route_added", payload={"key": "val"})
        assert e.module == "router"
        assert e.event_type == "route_added"
        assert e.payload == {"key": "val"}


class TestErrorEvent:
    def test_from_dict(self):
        r = ErrorEvent.from_dict(
            {"category": "spawn_failure", "member_id": "a1", "error": "profile not found"}
        )
        assert r.category == "spawn_failure"
        assert r.context["member_id"] == "a1"
        assert r.context["error"] == "profile not found"
        assert "a1" in r.message
        assert "profile not found" in r.message


class TestEventQuery:
    def test_to_dict_all_fields(self):
        q = EventQuery(
            since_ms=100,
            until_ms=200,
            member_id="agent-1",
            event_types=["run_completed"],
            limit=10,
            after_seq=5,
        )
        d = q.to_dict()
        assert d["since_ms"] == 100
        assert d["until_ms"] == 200
        assert d["member_id"] == "agent-1"
        assert d["event_types"] == ["run_completed"]
        assert d["limit"] == 10
        assert d["after_seq"] == 5

    def test_to_dict_empty(self):
        q = EventQuery()
        d = q.to_dict()
        assert d == {}

    def test_to_dict_partial(self):
        q = EventQuery(since_ms=100, limit=5)
        d = q.to_dict()
        assert d == {"since_ms": 100, "limit": 5}


class TestMemberStateConstants:
    def test_member_state_active(self):
        assert MEMBER_STATE_ACTIVE == "active"

    def test_member_state_retiring(self):
        assert MEMBER_STATE_RETIRING == "retiring"
