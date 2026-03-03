"""MobKit builder chain for runtime configuration."""
from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Protocol


class DiscoveryCallback(Protocol):
    async def __call__(self) -> list[dict[str, Any]]:
        ...


class PreSpawnCallback(Protocol):
    async def __call__(self) -> None:
        ...


@dataclass
class MobKitBuilderConfig:
    """Accumulated configuration from the builder chain."""

    mob_config_path: str | None = None
    session_builder: Any | None = None
    session_store: Any | None = None
    discovery_callback: DiscoveryCallback | None = None
    pre_spawn_callback: PreSpawnCallback | None = None
    gating_config_path: str | None = None
    routing_config_path: str | None = None
    scheduling_config_path: str | None = None
    memory_config: Any | None = None
    auth_config: Any | None = None
    gateway_bin: str | None = None
    modules: list[dict[str, Any]] = field(default_factory=list)


class MobKitBuilder:
    """Chainable builder for MobKit runtime configuration.

    Usage::

        runtime = await (
            MobKit.builder()
            .mob("config/mob.toml")
            .session_service(builder, store)
            .discovery(discover_fn)
            .build()
        )
    """

    def __init__(self) -> None:
        self._config = MobKitBuilderConfig()

    def mob(self, config_path: str) -> MobKitBuilder:
        """Set the mob configuration TOML path."""
        self._config.mob_config_path = config_path
        return self

    def session_service(self, builder: Any, store: Any = None) -> MobKitBuilder:
        """Set the session agent builder and optional store."""
        self._config.session_builder = builder
        self._config.session_store = store
        return self

    def discovery(self, callback: DiscoveryCallback) -> MobKitBuilder:
        """Set the discovery callback for agent specs."""
        self._config.discovery_callback = callback
        return self

    def pre_spawn(self, callback: PreSpawnCallback) -> MobKitBuilder:
        """Set the pre-spawn hook (runs before discovery)."""
        self._config.pre_spawn_callback = callback
        return self

    def gating(self, config_path: str) -> MobKitBuilder:
        """Set the gating configuration TOML path."""
        self._config.gating_config_path = config_path
        return self

    def routing(self, config_path: str) -> MobKitBuilder:
        """Set the routing configuration TOML path."""
        self._config.routing_config_path = config_path
        return self

    def scheduling(self, config_path: str) -> MobKitBuilder:
        """Set the scheduling configuration TOML path."""
        self._config.scheduling_config_path = config_path
        return self

    def memory(self, config: Any) -> MobKitBuilder:
        """Set the memory backend configuration."""
        self._config.memory_config = config
        return self

    def auth(self, config: Any) -> MobKitBuilder:
        """Set the auth configuration."""
        self._config.auth_config = config
        return self

    def gateway(self, bin_path: str) -> MobKitBuilder:
        """Set the path to the mobkit-rpc gateway binary."""
        self._config.gateway_bin = bin_path
        return self

    def modules(self, module_specs: list[dict[str, Any]]) -> MobKitBuilder:
        """Set module specifications."""
        self._config.modules = module_specs
        return self

    async def build(self) -> MobKitRuntime:
        """Build and start the MobKit runtime."""
        from .runtime import MobKitRuntime

        return await MobKitRuntime._create(self._config)


class MobKit:
    """Entry point for building MobKit runtimes."""

    @staticmethod
    def builder() -> MobKitBuilder:
        """Create a new MobKit builder."""
        return MobKitBuilder()
