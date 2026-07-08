"""Tests for builder chain."""
import os
from pathlib import Path

import pytest
from meerkat_mobkit.builder import MobKit, MobKitBuilder
from meerkat_mobkit.runtime import MobKitRuntime


class TestBuilderChain:
    def test_builder_returns_builder(self):
        b = MobKit.builder()
        assert isinstance(b, MobKitBuilder)

    def test_mob_returns_builder(self):
        b = MobKit.builder().mob("config/mob.toml")
        assert isinstance(b, MobKitBuilder)

    def test_mob_sets_config_path(self):
        b = MobKit.builder().mob("config/mob.toml")
        assert b._config.mob_config_path == "config/mob.toml"

    def test_implicit_delegate_idle_retirement_sets_runtime_option(self):
        b = MobKit.builder().implicit_delegate_idle_retirement(30)
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["implicit_delegate_idle_retire_secs"] == 30

    def test_implicit_delegate_idle_retirement_can_be_disabled(self):
        b = MobKit.builder().implicit_delegate_idle_retirement(None)
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["implicit_delegate_idle_retire_secs"] is None

    def test_implicit_delegate_idle_retirement_rejects_negative_seconds(self):
        with pytest.raises(ValueError):
            MobKit.builder().implicit_delegate_idle_retirement(-1)

    def test_console_fetch_timeout_ms_sets_runtime_option(self):
        b = MobKit.builder().console_fetch_timeout_ms(120_000)
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["console_fetch_timeout_ms"] == 120_000

    def test_console_read_only_sets_runtime_option(self):
        b = MobKit.builder().console_read_only()
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["console_read_only"] is True

    def test_workgraph_defaults_to_omitted(self):
        b = MobKit.builder()
        params = MobKitRuntime(b._config)._build_init_params()

        assert "workgraph" not in params["runtime_options"]

    def test_workgraph_enabled_sets_runtime_option(self):
        b = MobKit.builder().workgraph()
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["workgraph"] is True

    def test_workgraph_disabled_sets_runtime_option(self):
        b = MobKit.builder().workgraph(False)
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["workgraph"] is False

    def test_workgraph_accepts_durable_store_directory_string(self):
        b = MobKit.builder().workgraph("/var/lib/mob/workgraph")
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["workgraph"] == "/var/lib/mob/workgraph"

    def test_console_fetch_timeout_ms_rejects_non_positive_values(self):
        with pytest.raises(ValueError):
            MobKit.builder().console_fetch_timeout_ms(0)

    def test_memory_stores_without_gateway_config_is_rejected(self):
        with pytest.raises(ValueError, match="not supported by the Rust gateway"):
            MobKit.builder().memory(stores=["main"])

    def test_agent_memory_default_reaches_runtime_options(self):
        b = MobKit.builder().agent_memory()
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["agent_memory"] is True

    def test_agent_memory_options_serialize_to_gateway_wire_keys(self):
        b = MobKit.builder().agent_memory(
            realm="family",
            selection="contextual",
            max_entries=3,
            recall_timeout_ms=1200,
            recall_failure_policy="fail",
            instruction_header="Remember",
        )
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["agent_memory"] == {
            "realm": "family",
            "selection": "contextual",
            "max_entries": 3,
            "recall_timeout_ms": 1200,
            "recall_failure_policy": "fail",
            "instruction_header": "Remember",
        }

    def test_agent_memory_disable_reaches_runtime_options(self):
        b = MobKit.builder().agent_memory(False)
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["agent_memory"] == {"enabled": False}

    def test_agent_memory_taint_knobs_serialize_to_gateway_wire_keys(self):
        b = MobKit.builder().agent_memory(
            llm_writes="quarantined",
            recorder_tool=False,
            content_trust={
                "trusted_mcp_servers": ["knowledge_graph"],
                "untrusted_tools": ["scrape_page"],
            },
        )
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["agent_memory"] == {
            "llm_writes": "quarantined",
            "recorder_tool": False,
            "content_trust": {
                "trusted_mcp_servers": ["knowledge_graph"],
                "untrusted_tools": ["scrape_page"],
            },
        }

    def test_agent_memory_taint_knobs_accept_camel_case(self):
        b = MobKit.builder().agent_memory(
            llmWrites="quarantined",
            recorderTool=True,
            contentTrust={"trusted_tools": ["safe_calc"]},
        )
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["agent_memory"] == {
            "llm_writes": "quarantined",
            "recorder_tool": True,
            "content_trust": {"trusted_tools": ["safe_calc"]},
        }

    def test_agent_memory_selector_serializes_to_gateway_wire_key(self):
        b = MobKit.builder().agent_memory(selector="profile:/etc/mobkit/selector.toml")
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["agent_memory"] == {
            "selector": "profile:/etc/mobkit/selector.toml",
        }

    def test_agent_memory_distiller_serializes_to_gateway_wire_keys(self):
        b = MobKit.builder().agent_memory(
            distiller={
                "enabled": True,
                "runs_per_hour": 6,
                "min_interactions": 5,
                "model": "claude-haiku-4-5",
            },
        )
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["agent_memory"] == {
            "distiller": {
                "enabled": True,
                "runs_per_hour": 6,
                "min_interactions": 5,
                "model": "claude-haiku-4-5",
            },
        }

    def test_agent_memory_distiller_accepts_camel_case_and_bool(self):
        b = MobKit.builder().agent_memory(
            distiller={"runsPerHour": 3, "minInteractions": 2},
        )
        params = MobKitRuntime(b._config)._build_init_params()
        assert params["runtime_options"]["agent_memory"] == {
            "distiller": {"runs_per_hour": 3, "min_interactions": 2},
        }

        b = MobKit.builder().agent_memory(distiller=True)
        params = MobKitRuntime(b._config)._build_init_params()
        assert params["runtime_options"]["agent_memory"] == {"distiller": True}

    def test_agent_memory_steward_serializes_to_gateway_wire_keys(self):
        b = MobKit.builder().agent_memory(
            steward={
                "enabled": True,
                "cadence": "*/6h",
                "model": "claude-sonnet-4-6",
                "per_mob": False,
                "runs_per_day": 4,
                "min_signals": 3,
            },
        )
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["runtime_options"]["agent_memory"] == {
            "steward": {
                "enabled": True,
                "cadence": "*/6h",
                "model": "claude-sonnet-4-6",
                "per_mob": False,
                "runs_per_day": 4,
                "min_signals": 3,
            },
        }

    def test_agent_memory_steward_accepts_camel_case_and_bool(self):
        b = MobKit.builder().agent_memory(
            steward={"runsPerDay": 2, "minSignals": 5, "perMob": True},
        )
        params = MobKitRuntime(b._config)._build_init_params()
        assert params["runtime_options"]["agent_memory"] == {
            "steward": {"per_mob": True, "runs_per_day": 2, "min_signals": 5},
        }

        b = MobKit.builder().agent_memory(steward=True)
        params = MobKitRuntime(b._config)._build_init_params()
        assert params["runtime_options"]["agent_memory"] == {"steward": True}

    def test_agent_memory_operator_scope_serializes_to_gateway_wire_key(self):
        b = MobKit.builder().agent_memory(store="sqlite", operator_scope="provisional")
        params = MobKitRuntime(b._config)._build_init_params()
        assert params["runtime_options"]["agent_memory"] == {
            "store": "sqlite",
            "operator_scope": "provisional",
        }

    def test_agent_memory_operator_scope_accepts_camel_case(self):
        b = MobKit.builder().agent_memory(operatorScope="off")
        params = MobKitRuntime(b._config)._build_init_params()
        assert params["runtime_options"]["agent_memory"] == {"operator_scope": "off"}

    def test_agent_memory_hygienist_serializes_to_gateway_wire_keys(self):
        b = MobKit.builder().agent_memory(
            hygienist={
                "enabled": True,
                "runs_per_day": 3,
                "model": "claude-haiku-4-5",
            },
        )
        params = MobKitRuntime(b._config)._build_init_params()
        assert params["runtime_options"]["agent_memory"] == {
            "hygienist": {
                "enabled": True,
                "runs_per_day": 3,
                "model": "claude-haiku-4-5",
            },
        }

    def test_agent_memory_hygienist_accepts_camel_case_and_bool(self):
        b = MobKit.builder().agent_memory(hygienist={"runsPerDay": 2})
        params = MobKitRuntime(b._config)._build_init_params()
        assert params["runtime_options"]["agent_memory"] == {
            "hygienist": {"runs_per_day": 2},
        }

        b = MobKit.builder().agent_memory(hygienist=True)
        params = MobKitRuntime(b._config)._build_init_params()
        assert params["runtime_options"]["agent_memory"] == {"hygienist": True}

    def test_agent_memory_unknown_option_raises_instead_of_silently_dropping(self):
        with pytest.raises(ValueError, match="per_turn_injecton"):
            MobKit.builder().agent_memory(per_turn_injecton="budgeted")

    def test_agent_memory_unknown_nested_option_raises(self):
        with pytest.raises(ValueError, match="distiller.*runsperhour_typo"):
            MobKit.builder().agent_memory(
                distiller={"runs_per_hour": 2, "runsperhour_typo": 9},
            )
        with pytest.raises(ValueError, match="steward.*cadance"):
            MobKit.builder().agent_memory(steward={"cadance": "*/6h"})
        with pytest.raises(ValueError, match="hygienist.*runs_per_dya"):
            MobKit.builder().agent_memory(hygienist={"runs_per_dya": 2})

    def test_external_authoritative_path_requires_all_three_parts(self):
        class Store:
            pass

        b = MobKit.builder().continuity_store(Store())
        with pytest.raises(ValueError, match="lease_provider.*scratch_dir"):
            b._validate()

    def test_external_provider_init_params_include_flags_and_scratch_dir(self):
        class Store:
            pass

        class Lease:
            pass

        b = (
            MobKit.builder()
            .continuity_store(Store())
            .lease_provider(Lease())
            .scratch_dir("/tmp/mobkit-scratch")
        )
        params = MobKitRuntime(b._config)._build_init_params()

        assert params["has_continuity_store"] is True
        assert params["has_lease_provider"] is True
        assert params["scratch_dir"] == "/tmp/mobkit-scratch"


class TestConventionDefaults:
    def test_gating_discovered(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        (tmp_path / "config").mkdir()
        (tmp_path / "config" / "gating.toml").write_text("[[rules]]")

        b = MobKit.builder().mob("config/mob.toml")
        b._apply_convention_defaults()
        assert b._config.gating_config_path == "config/gating.toml"

    def test_access_discovered(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        (tmp_path / "config").mkdir()
        (tmp_path / "config" / "access.toml").write_text("enabled = false")

        b = MobKit.builder().mob("config/mob.toml")
        b._apply_convention_defaults()
        assert b._config.access_config_path == "config/access.toml"

    def test_explicit_access_overrides_convention(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        (tmp_path / "config").mkdir()
        (tmp_path / "config" / "access.toml").write_text("conventional")

        b = MobKit.builder().mob("config/mob.toml").access_control("custom/access.toml")
        b._apply_convention_defaults()
        assert b._config.access_config_path == "custom/access.toml"

    def test_access_config_path_reaches_runtime_options(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)

        b = MobKit.builder().mob_inline("[mob]\nid = \"t\"").access_control("config/access.toml")
        params = MobKitRuntime(b._config)._build_init_params()
        assert params["runtime_options"]["access_config_path"] == "config/access.toml"

    def test_routing_discovered(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        (tmp_path / "deployment").mkdir()
        (tmp_path / "deployment" / "routing.toml").write_text("[[routes]]")

        b = MobKit.builder().mob("config/mob.toml")
        b._apply_convention_defaults()
        assert b._config.routing_config_path == "deployment/routing.toml"

    def test_scheduling_discovered(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        (tmp_path / "config" / "defaults").mkdir(parents=True)
        (tmp_path / "deployment").mkdir()
        (tmp_path / "config" / "defaults" / "schedules.toml").write_text("default")
        (tmp_path / "deployment" / "schedules.toml").write_text("override")

        b = MobKit.builder().mob("config/mob.toml")
        b._apply_convention_defaults()
        assert len(b._config.scheduling_files) == 2
        assert "defaults" in b._config.scheduling_files[0]
        assert "deployment" in b._config.scheduling_files[1]

    def test_missing_files_skipped(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)

        b = MobKit.builder().mob("config/mob.toml")
        b._apply_convention_defaults()
        assert b._config.gating_config_path is None
        assert b._config.routing_config_path is None
        assert b._config.scheduling_files == []

    def test_explicit_overrides_convention(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        (tmp_path / "config").mkdir()
        (tmp_path / "config" / "gating.toml").write_text("conventional")

        b = MobKit.builder().mob("config/mob.toml").gating("custom/gating.toml")
        b._apply_convention_defaults()
        assert b._config.gating_config_path == "custom/gating.toml"

    def test_explicit_scheduling_overrides_convention(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        (tmp_path / "config" / "defaults").mkdir(parents=True)
        (tmp_path / "config" / "defaults" / "schedules.toml").write_text("conventional")

        b = MobKit.builder().mob("config/mob.toml").scheduling("custom/s.toml")
        b._apply_convention_defaults()
        assert b._config.scheduling_files == ["custom/s.toml"]

