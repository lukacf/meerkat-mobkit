"""Tests for data models (SessionBuildOptions, DiscoverySpec, etc.)."""
import pytest

from meerkat_mobkit.models import (
    DiscoverySpec,
    PreSpawnData,
    SessionBuildOptions,
    SessionQuery,
)


class TestSessionBuildOptions:
    def test_add_tools(self):
        opts = SessionBuildOptions()
        opts.add_tools(["tool_a", "tool_b"])
        assert opts.tools == ["tool_a", "tool_b"]

    def test_add_tools_non_string_raises(self):
        opts = SessionBuildOptions()
        with pytest.raises(TypeError):
            opts.add_tools([123])

    def test_to_dict(self):
        opts = SessionBuildOptions(
            app_context={"k": "v"},
            session_id="s-1",
            labels={"env": "test"},
            profile_name="default",
        )
        opts.add_tools(["tool_x"])
        d = opts.to_dict()
        assert d["app_context"] == {"k": "v"}
        assert d["session_id"] == "s-1"
        assert d["labels"] == {"env": "test"}
        assert d["profile_name"] == "default"
        assert d["tools"] == ["tool_x"]


class TestDiscoverySpec:
    def test_to_dict(self):
        spec = DiscoverySpec(
            profile="prof-1",
            meerkat_id="m-1",
            labels={"role": "worker"},
        )
        d = spec.to_dict()
        assert d["profile"] == "prof-1"
        assert d["meerkat_id"] == "m-1"
        assert d["labels"] == {"role": "worker"}


class TestPreSpawnData:
    def test_to_dict(self):
        ps = PreSpawnData(resume_map={"m-1": "s-1"}, module_id="mod-1")
        d = ps.to_dict()
        assert d["resume_map"] == {"m-1": "s-1"}
        assert d["module_id"] == "mod-1"


class TestSessionQuery:
    def test_to_dict(self):
        q = SessionQuery(agent_type="chat", limit=50)
        d = q.to_dict()
        assert d["agent_type"] == "chat"
        assert d["limit"] == 50
        assert d["include_deleted"] is False
