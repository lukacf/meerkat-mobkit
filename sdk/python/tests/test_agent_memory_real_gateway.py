"""Real gateway smoke tests for Python agent-memory helpers.

This covers the Python SDK -> rpc_gateway -> Rust agent-memory provider path
without requiring an LLM provider key. It boots an identity-first runtime,
writes a durable memory record, recalls it, verifies the markdown store, and
then deletes it.

Run:
    scripts/repo-cargo build -p meerkat-mobkit --bin rpc_gateway
    PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests/test_agent_memory_real_gateway.py -q
"""
from __future__ import annotations

import os
from pathlib import Path
import subprocess

import pytest

from meerkat_mobkit.builder import MobKit
from meerkat_mobkit.identity_first_models import DurableAgentSpec


def _resolve_gateway_bin() -> str:
    override = os.environ.get("MOBKIT_GATEWAY_BIN", "").strip()
    if override:
        return override
    repo_root = Path(__file__).resolve().parents[3]
    try:
        result = subprocess.run(
            [str(repo_root / "scripts" / "repo-cargo"), "--print-env", "CARGO_TARGET_DIR"],
            cwd=repo_root,
            check=True,
            capture_output=True,
            text=True,
        )
    except (OSError, subprocess.CalledProcessError):
        return ""
    for line in result.stdout.splitlines():
        if line.startswith("CARGO_TARGET_DIR="):
            target_dir = line.split("=", 1)[1].strip()
            if target_dir:
                return str(Path(target_dir) / "debug" / "rpc_gateway")
    return ""


_GATEWAY_BIN = _resolve_gateway_bin()

_skip_no_binary = pytest.mark.skipif(
    not _GATEWAY_BIN or not os.path.isfile(_GATEWAY_BIN),
    reason=(
        f"Gateway binary not found at {_GATEWAY_BIN} - run: "
        "./scripts/repo-cargo build -p meerkat-mobkit --bin rpc_gateway"
    ),
)


_MOB_TOML = """\
[mob]
id = "agent-memory-python-smoke"

[profiles.memory_smoke]
model = "gpt-5.5"
system_prompt = "You are a deterministic agent-memory smoke-test profile."
external_addressable = true

[profiles.memory_smoke.tools]
comms = true
"""


class SmokeRoster:
    async def roster(self, context: dict) -> list[DurableAgentSpec]:
        return [
            DurableAgentSpec(
                identity="identity:memory-smoke",
                profile="memory_smoke",
                addressability="addressable",
                labels={"topic": "memory-smoke"},
                additional_instructions=["Keep smoke-test answers terse."],
            )
        ]


@_skip_no_binary
@pytest.mark.asyncio
@pytest.mark.timeout(60)
async def test_python_agent_memory_helpers_round_trip_through_real_gateway(tmp_path):
    mob_toml = tmp_path / "mob.toml"
    mob_toml.write_text(_MOB_TOML)
    state_dir = tmp_path / "state"
    state_dir.mkdir()

    runtime = await (
        MobKit.builder()
        .gateway(_GATEWAY_BIN)
        .mob(str(mob_toml))
        .persistent_state(str(state_dir))
        .roster(SmokeRoster())
        # The gateway default store is sqlite; this test selects markdown
        # explicitly to verify both the store passthrough and the markdown
        # file layout end to end.
        .agent_memory(selection="contextual", max_entries=4, store="markdown")
        .build()
    )
    try:
        handle = runtime.mob_handle()
        record = await handle.remember_agent_memory(
            "identity:memory-smoke",
            title="Python smoke token",
            body="The Python SDK memory smoke token is PY-MEM-17.",
            tags=["python", "smoke"],
        )
        assert record.memory_id.startswith("mem-")
        assert record.title == "Python smoke token"
        assert record.tags == ["python", "smoke"]

        recalled = await handle.recall_agent_memory(
            "identity:memory-smoke",
            selection="contextual",
            query_text="Where is the PY-MEM-17 token?",
            query_terms=["PY-MEM-17"],
            max_entries=4,
        )
        assert [item.memory_id for item in recalled] == [record.memory_id]
        assert recalled[0].body == "The Python SDK memory smoke token is PY-MEM-17."

        memory_file = (
            state_dir
            / "agent-memory"
            / "default"
            / "identity%3Amemory-smoke.md"
        )
        written = memory_file.read_text()
        assert "PY-MEM-17" in written
        assert record.memory_id in written

        forgotten = await handle.forget_agent_memory(
            "identity:memory-smoke", record.memory_id
        )
        assert forgotten.memory_id == record.memory_id
        assert forgotten.deleted is True

        after_forget = await handle.recall_agent_memory(
            "identity:memory-smoke",
            selection="always",
            max_entries=4,
        )
        assert after_forget == []
        assert "PY-MEM-17" not in memory_file.read_text()
    finally:
        await runtime.shutdown()


@_skip_no_binary
@pytest.mark.asyncio
@pytest.mark.timeout(60)
async def test_python_agent_memory_defaults_to_sqlite_store(tmp_path):
    mob_toml = tmp_path / "mob.toml"
    mob_toml.write_text(_MOB_TOML)
    state_dir = tmp_path / "state"
    state_dir.mkdir()

    runtime = await (
        MobKit.builder()
        .gateway(_GATEWAY_BIN)
        .mob(str(mob_toml))
        .persistent_state(str(state_dir))
        .roster(SmokeRoster())
        .agent_memory(selection="contextual", max_entries=4)
        .build()
    )
    try:
        handle = runtime.mob_handle()
        record = await handle.remember_agent_memory(
            "identity:memory-smoke",
            title="Sqlite smoke token",
            body="The sqlite default-store smoke token is PY-MEM-18.",
            tags=["python", "smoke"],
        )
        assert record.memory_id.startswith("mem-")

        recalled = await handle.recall_agent_memory(
            "identity:memory-smoke",
            selection="contextual",
            query_text="Where is the PY-MEM-18 token?",
            query_terms=["PY-MEM-18"],
            max_entries=4,
        )
        assert [item.memory_id for item in recalled] == [record.memory_id]

        sqlite_file = state_dir / "agent-memory" / "default.sqlite3"
        assert sqlite_file.is_file(), "default store must be the per-realm sqlite database"
        markdown_file = (
            state_dir / "agent-memory" / "default" / "identity%3Amemory-smoke.md"
        )
        assert not markdown_file.exists(), "sqlite default must not write markdown files"
    finally:
        await runtime.shutdown()
