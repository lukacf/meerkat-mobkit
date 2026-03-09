"""Tests for builder chain."""
import os
from pathlib import Path

import pytest
from meerkat_mobkit.builder import MobKit, MobKitBuilder


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


class TestConventionDefaults:
    def test_gating_discovered(self, tmp_path, monkeypatch):
        monkeypatch.chdir(tmp_path)
        (tmp_path / "config").mkdir()
        (tmp_path / "config" / "gating.toml").write_text("[[rules]]")

        b = MobKit.builder().mob("config/mob.toml")
        b._apply_convention_defaults()
        assert b._config.gating_config_path == "config/gating.toml"

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
