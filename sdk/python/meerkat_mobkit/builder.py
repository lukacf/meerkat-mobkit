"""MobKit builder chain — matches HomeCore's app.py patterns."""
from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Awaitable, Callable, Sequence


@dataclass
class MobKitBuilderConfig:
    mob_config_path: str | None = None
    session_builder: Any | None = None
    session_store: Any | None = None
    discovery_callback: Any | None = None
    pre_spawn_callback: Any | None = None
    gating_config_path: str | None = None
    routing_config_path: str | None = None
    scheduling_files: list[str] = field(default_factory=list)
    memory_config: Any | None = None
    auth_config: Any | None = None
    gateway_bin: str | None = None
    modules: list[dict[str, Any]] = field(default_factory=list)
    extra_routes: Any | None = None


class MobKitBuilder:
    """Chainable builder for MobKit runtime configuration.

    Usage:
        runtime = await (
            MobKit.builder()
            .mob("config/mob.toml")
            .session_service(builder, store)
            .discovery(discover_fn)
            .scheduling("schedules/a.toml", "schedules/b.toml")
            .build()
        )
    """

    def __init__(self) -> None:
        self._config = MobKitBuilderConfig()

    def mob(self, config_path: str) -> MobKitBuilder:
        self._config.mob_config_path = config_path
        return self

    def session_service(self, builder: Any, store: Any = None) -> MobKitBuilder:
        self._config.session_builder = builder
        self._config.session_store = store
        return self

    def discovery(self, callback: Any) -> MobKitBuilder:
        self._config.discovery_callback = callback
        return self

    def pre_spawn(self, callback: Any) -> MobKitBuilder:
        self._config.pre_spawn_callback = callback
        return self

    def gating(self, config_path: str) -> MobKitBuilder:
        self._config.gating_config_path = config_path
        return self

    def routing(self, config_path: str) -> MobKitBuilder:
        self._config.routing_config_path = config_path
        return self

    def scheduling(self, *schedule_files: str) -> MobKitBuilder:
        """Set schedule config files (accepts multiple positional args)."""
        self._config.scheduling_files = list(schedule_files)
        return self

    def memory(self, config: Any = None, *, stores: list[str] | None = None) -> MobKitBuilder:
        """Set memory config. Accepts config object or stores=["knowledge_graph", ...]."""
        self._config.memory_config = config or {"stores": stores or []}
        return self

    def auth(self, config: Any) -> MobKitBuilder:
        self._config.auth_config = config
        return self

    def gateway(self, bin_path: str) -> MobKitBuilder:
        self._config.gateway_bin = bin_path
        return self

    def modules(self, module_specs: list[dict[str, Any]]) -> MobKitBuilder:
        self._config.modules = module_specs
        return self

    async def build(self) -> MobKitRuntime:
        self._apply_convention_defaults()
        from .runtime import MobKitRuntime
        return await MobKitRuntime._create(self._config)

    def _apply_convention_defaults(self) -> None:
        """Fill in conventional config paths when not explicitly set.

        Convention (relative to cwd):
        - config/gating.toml → gating config
        - config/defaults/schedules.toml → default schedules
        - deployment/routing.toml → routing config
        - deployment/schedules.toml → deployment schedule overrides

        Only checks when the corresponding builder method was NOT called.
        Explicit paths always win. Files that don't exist are skipped.
        """
        if self._config.gating_config_path is None:
            candidate = Path("config/gating.toml")
            if candidate.is_file():
                self._config.gating_config_path = str(candidate)

        if self._config.routing_config_path is None:
            candidate = Path("deployment/routing.toml")
            if candidate.is_file():
                self._config.routing_config_path = str(candidate)

        if not self._config.scheduling_files:
            files: list[str] = []
            default = Path("config/defaults/schedules.toml")
            if default.is_file():
                files.append(str(default))
            override = Path("deployment/schedules.toml")
            if override.is_file():
                files.append(str(override))
            if files:
                self._config.scheduling_files = files


class MobKit:
    @staticmethod
    def builder() -> MobKitBuilder:
        return MobKitBuilder()
