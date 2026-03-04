"""Tests that the public surface is correct and legacy symbols are removed."""
import pytest
import meerkat_mobkit


class TestNewSymbolsExist:
    @pytest.mark.parametrize("name", [
        "MobKit", "MobKitBuilder", "MobKitRuntime",
        "DiscoverySpec", "PreSpawnData", "SessionBuildOptions", "SessionQuery",
        "SessionAgentBuilder",
        "MobKitError", "TransportError", "RpcError",
        "CapabilityUnavailableError", "ContractMismatchError", "NotConnectedError",
        "StatusResult", "CapabilitiesResult", "ReconcileResult",
        "SpawnResult", "SpawnMemberResult", "SubscribeResult",
        "KeepAliveConfig", "EventEnvelope",
        "RoutingResolution", "DeliveryResult", "MemoryQueryResult",
        "MobEvent", "AgentEvent", "InteractionEvent", "EventStream",
        "auth", "memory", "session_store",
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
        "CallbackDispatcher",
        "ModuleDefinition",
        "ModuleSpec",
        "ModuleTool",
        "build_console_experience_route",
        "build_console_modules_route",
        "build_console_route",
        "build_console_routes",
        "build_module_spec",
        "decorate_module_spec",
        "decorate_module_tool",
        "define_module",
        "define_module_spec",
        "define_module_tool",
    ])
    def test_symbol_removed(self, name):
        assert not hasattr(meerkat_mobkit, name), f"{name} should NOT be in public surface"


class TestHelpersStillImportable:
    """Module authoring helpers are importable via meerkat_mobkit.helpers."""

    def test_helpers_importable(self):
        from meerkat_mobkit.helpers import (
            ModuleDefinition,
            ModuleSpec,
            ModuleTool,
            build_module_spec,
            define_module,
            define_module_tool,
        )
        assert ModuleSpec is not None
