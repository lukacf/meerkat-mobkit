"""MobKit builder chain — matches HomeCore's app.py patterns."""
from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any, Awaitable, Callable, Sequence


@dataclass
class MobKitBuilderConfig:
    mob_config_path: str | None = None
    mob_config_inline: str | None = None
    session_builder: Any | None = None
    session_store: Any | None = None
    discovery_callback: Any | None = None
    pre_spawn_callback: Any | None = None
    error_callback: Any | None = None
    event_log: Any | None = None
    console_read_only: bool | None = None
    console_fetch_timeout_ms: int | None = None
    gating_config_path: str | None = None
    access_config_path: str | None = None
    routing_config_path: str | None = None
    scheduling_files: list[str] = field(default_factory=list)
    memory_config: Any | None = None
    agent_memory_config: Any | None = None
    auth_config: Any | None = None
    implicit_delegate_idle_retire_secs: int | None = None
    implicit_delegate_idle_retire_configured: bool = False
    gateway_bin: str | None = None
    modules: list[dict[str, Any]] = field(default_factory=list)
    extra_routes: Any | None = None
    persistent_state: str | None = None
    # Identity-first provider fields (REQ-44)
    continuity_store: Any | None = None
    lease_provider: Any | None = None
    scratch_dir: str | None = None
    roster_provider: Any | None = None
    topology_provider: Any | None = None
    agent_customizer: Any | None = None


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
        """Set the mob definition from a TOML file path."""
        self._config.mob_config_path = config_path
        return self

    def mob_inline(self, toml_content: str) -> MobKitBuilder:
        """Set the mob definition from an inline TOML string.

        Mutually exclusive with mob(config_path).
        """
        self._config.mob_config_inline = toml_content
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

    def event_log(self, *, storage: Any, **kwargs: Any) -> MobKitBuilder:
        """Configure persistent event log.

        Args:
            storage: An EventLogStore implementation for the app's backend.
            **kwargs: Additional config (batch_size, flush_interval_ms, filter).
        """
        self._config.event_log = {"storage": storage, **kwargs}
        return self

    def on_error(self, callback: Callable[..., Any] | Callable[..., Awaitable[Any]]) -> MobKitBuilder:
        """Register an error hook for operational alerting.

        The callback receives an ``ErrorEvent`` and is fire-and-forget.
        It can be sync or async::

            async def on_error(event: ErrorEvent):
                await slack.post(f"[{event.category}] {event.message}")

            rt = await MobKit.builder().mob("mob.toml").on_error(on_error).build()
        """
        self._config.error_callback = callback
        return self

    def gating(self, config_path: str) -> MobKitBuilder:
        self._config.gating_config_path = config_path
        return self

    def access_control(self, config_path: str) -> MobKitBuilder:
        """Enable ABAC access control backed by a TOML file.

        Conventionally ``config/access.toml`` (auto-discovered when present).
        A missing file starts disabled; console admin edits persist back to
        the same path. Without this (and without a conventional
        ``config/access.toml``) access control is off entirely.
        """
        self._config.access_config_path = config_path
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
        if config is None and stores is not None:
            raise ValueError(
                "memory(stores=...) is not supported by the Rust gateway; "
                "pass memory.local_json()"
            )
        self._config.memory_config = config
        return self

    def agent_memory(self, config: Any = True, **kwargs: Any) -> MobKitBuilder:
        """Configure identity-scoped durable agent memory.

        With no arguments this enables the gateway default. Keyword arguments
        use Python names and serialize to the Rust gateway's snake_case wire
        keys, for example ``agent_memory(selection="contextual", max_entries=3)``.
        """
        if kwargs:
            if config is not True:
                raise ValueError("agent_memory accepts either config or keyword options")
            config = kwargs
        if config is False or config is None:
            self._config.agent_memory_config = {"enabled": False}
            return self
        if config is True:
            self._config.agent_memory_config = True
            return self
        if not isinstance(config, dict):
            self._config.agent_memory_config = config
            return self

        wire: dict[str, Any] = {}
        if "enabled" in config:
            wire["enabled"] = config["enabled"]
        if "realm" in config:
            wire["realm"] = config["realm"]
        if "selection" in config:
            wire["selection"] = config["selection"]
        if "max_entries" in config:
            wire["max_entries"] = config["max_entries"]
        elif "maxEntries" in config:
            wire["max_entries"] = config["maxEntries"]
        if "recall_timeout_ms" in config:
            wire["recall_timeout_ms"] = config["recall_timeout_ms"]
        elif "recallTimeoutMs" in config:
            wire["recall_timeout_ms"] = config["recallTimeoutMs"]
        if "recall_failure_policy" in config:
            wire["recall_failure_policy"] = config["recall_failure_policy"]
        elif "recallFailurePolicy" in config:
            wire["recall_failure_policy"] = config["recallFailurePolicy"]
        if "instruction_header" in config:
            wire["instruction_header"] = config["instruction_header"]
        elif "instructionHeader" in config:
            wire["instruction_header"] = config["instructionHeader"]
        if "per_turn_injection" in config:
            wire["per_turn_injection"] = config["per_turn_injection"]
        elif "perTurnInjection" in config:
            wire["per_turn_injection"] = config["perTurnInjection"]
        if "defang_inbound" in config:
            wire["defang_inbound"] = config["defang_inbound"]
        elif "defangInbound" in config:
            wire["defang_inbound"] = config["defangInbound"]
        if "store" in config:
            wire["store"] = config["store"]
        if "llm_writes" in config:
            wire["llm_writes"] = config["llm_writes"]
        elif "llmWrites" in config:
            wire["llm_writes"] = config["llmWrites"]
        if "recorder_tool" in config:
            wire["recorder_tool"] = config["recorder_tool"]
        elif "recorderTool" in config:
            wire["recorder_tool"] = config["recorderTool"]
        if "content_trust" in config:
            wire["content_trust"] = config["content_trust"]
        elif "contentTrust" in config:
            wire["content_trust"] = config["contentTrust"]
        if "selector" in config:
            wire["selector"] = config["selector"]
        distiller = config.get("distiller")
        if distiller is not None:
            if isinstance(distiller, dict):
                distiller_wire: dict[str, Any] = {}
                if "enabled" in distiller:
                    distiller_wire["enabled"] = distiller["enabled"]
                if "runs_per_hour" in distiller:
                    distiller_wire["runs_per_hour"] = distiller["runs_per_hour"]
                elif "runsPerHour" in distiller:
                    distiller_wire["runs_per_hour"] = distiller["runsPerHour"]
                if "min_interactions" in distiller:
                    distiller_wire["min_interactions"] = distiller["min_interactions"]
                elif "minInteractions" in distiller:
                    distiller_wire["min_interactions"] = distiller["minInteractions"]
                if "model" in distiller:
                    distiller_wire["model"] = distiller["model"]
                wire["distiller"] = distiller_wire
            else:
                wire["distiller"] = distiller
        self._config.agent_memory_config = wire
        return self

    def auth(self, config: Any) -> MobKitBuilder:
        self._config.auth_config = config
        return self

    def console_fetch_timeout_ms(self, timeout_ms: int) -> MobKitBuilder:
        if not isinstance(timeout_ms, int) or timeout_ms <= 0:
            raise ValueError("console_fetch_timeout_ms must be a positive integer")
        self._config.console_fetch_timeout_ms = timeout_ms
        return self

    def console_read_only(self, read_only: bool = True) -> MobKitBuilder:
        self._config.console_read_only = bool(read_only)
        return self

    def implicit_delegate_idle_retirement(
        self, seconds: int | None
    ) -> MobKitBuilder:
        """Configure idle auto-retirement for implicit delegation members.

        The default is owned by the gateway. Pass ``None`` to disable automatic
        retirement for runtimes that intentionally keep implicit delegates warm.
        """
        if seconds is not None and seconds < 0:
            raise ValueError(
                "implicit delegate idle retirement seconds must be non-negative or None"
            )
        self._config.implicit_delegate_idle_retire_secs = seconds
        self._config.implicit_delegate_idle_retire_configured = True
        return self

    def gateway(self, bin_path: str) -> MobKitBuilder:
        self._config.gateway_bin = bin_path
        return self

    def modules(self, module_specs: list[dict[str, Any]]) -> MobKitBuilder:
        self._config.modules = module_specs
        return self

    def persistent_state(self, path: str) -> MobKitBuilder:
        """Enable persistent state at the given path.

        When set, the gateway creates SQLite-backed session/runtime state,
        MobKit metadata, console logs, and binary blob storage under this
        directory. Mob storage remains in-memory. When not set, the gateway
        uses an ephemeral session service.
        """
        self._config.persistent_state = path
        return self

    def continuity_store(self, provider: Any) -> MobKitBuilder:
        """Set an external continuity store provider."""
        self._config.continuity_store = provider
        return self

    def lease_provider(self, provider: Any) -> MobKitBuilder:
        """Set an external lease provider."""
        self._config.lease_provider = provider
        return self

    def scratch_dir(self, path: str) -> MobKitBuilder:
        """Set the scratch directory for non-authoritative local state."""
        self._config.scratch_dir = path
        return self

    def roster(self, provider: Any) -> MobKitBuilder:
        """Set the roster provider for identity-first continuity."""
        self._config.roster_provider = provider
        return self

    def topology_provider(self, provider: Any) -> MobKitBuilder:
        """Set the topology provider for identity-first continuity."""
        self._config.topology_provider = provider
        return self

    def agent_customizer(self, customizer: Any) -> MobKitBuilder:
        """Set the agent customizer for identity-first continuity."""
        self._config.agent_customizer = customizer
        return self

    async def build(self) -> MobKitRuntime:
        self._validate()
        self._apply_convention_defaults()
        from .runtime import MobKitRuntime
        return await MobKitRuntime._create(self._config)

    def _validate(self) -> None:
        if self._config.mob_config_path and self._config.mob_config_inline:
            raise ValueError(
                "mob() and mob_inline() are mutually exclusive"
            )
        has_external = (
            self._config.continuity_store is not None
            or self._config.lease_provider is not None
            or self._config.scratch_dir is not None
        )
        if self._config.persistent_state and has_external:
            raise ValueError(
                "persistent_state and continuity_store/lease_provider/scratch_dir "
                "are mutually exclusive — use one path or the other"
            )
        if has_external:
            missing = []
            if self._config.continuity_store is None:
                missing.append("continuity_store")
            if self._config.lease_provider is None:
                missing.append("lease_provider")
            if self._config.scratch_dir is None:
                missing.append("scratch_dir")
            if missing:
                raise ValueError(
                    "external-authoritative path requires continuity_store() + "
                    "lease_provider() + scratch_dir(); missing: "
                    + ", ".join(missing)
                )

    def _apply_convention_defaults(self) -> None:
        """Fill in conventional config paths when not explicitly set.

        Convention (relative to cwd):
        - config/gating.toml → gating config
        - config/access.toml → ABAC access control config
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

        if self._config.access_config_path is None:
            candidate = Path("config/access.toml")
            if candidate.is_file():
                self._config.access_config_path = str(candidate)

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
