"""Tests that the public surface is correct and legacy symbols are removed."""
import pytest
import meerkat_mobkit


class TestNewSymbolsExist:
    @pytest.mark.parametrize("name", [
        "MobKit", "MobKitBuilder", "MobKitRuntime",
        "DiscoverySpec", "PreSpawnData", "SessionBuildOptions", "SessionQuery",
        "CallbackDispatcher", "SessionAgentBuilder",
        "MobKitError", "TransportError", "RpcError",
        "CapabilityUnavailableError", "ContractMismatchError", "NotConnectedError",
        "StatusResult", "CapabilitiesResult", "ReconcileResult",
        "SpawnResult", "SpawnMemberResult", "SubscribeResult",
        "RoutingResolution", "DeliveryResult", "MemoryQueryResult",
        "MobEvent", "AgentEvent", "InteractionEvent", "EventStream",
        "auth", "memory", "session_store",
        "ModuleDefinition", "ModuleSpec", "ModuleTool",
    ])
    def test_symbol_exists(self, name):
        assert hasattr(meerkat_mobkit, name), f"{name} should be in public surface"


class TestLegacySymbolsRemoved:
    @pytest.mark.parametrize("name", [
        "MobkitTypedClient",
        "MobkitAsyncTypedClient",
        "MobkitRpcError",
        "create_gateway_sync_transport",
        "create_gateway_async_transport",
        "create_http_transport",
        "PersistentTransport",
        "create_persistent_transport",
        "SseEvent",
        "SseEventStream",
        "parse_sse_stream",
        "MobHandle",
        "SseBridge",
    ])
    def test_symbol_removed(self, name):
        assert not hasattr(meerkat_mobkit, name), f"{name} should NOT be in public surface"
