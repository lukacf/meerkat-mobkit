"""Real gateway profile capability tests.

These tests cover the shipping Python SDK -> rpc_gateway -> MobKit/Meerkat
factory path. They intentionally assert the per-identity resolved tool surface,
not the global mobkit/tools/catalog.

Run:
    scripts/repo-cargo build -p meerkat-mobkit --bin rpc_gateway
    PYTHONPATH=sdk/python python3 -m pytest sdk/python/tests/test_profile_capabilities_real_gateway.py -q
"""
from __future__ import annotations

import os
from pathlib import Path
import subprocess

import pytest

from meerkat_mobkit.builder import MobKit
from meerkat_mobkit.errors import RpcError
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
_BROAD_BUILTIN_TOOLS = {
    "apply_patch",
    "browse_skills",
    "task_create",
    "task_list",
}

_skip_no_binary = pytest.mark.skipif(
    not _GATEWAY_BIN or not os.path.isfile(_GATEWAY_BIN),
    reason=(
        f"Gateway binary not found at {_GATEWAY_BIN} - run: "
        "./scripts/repo-cargo build -p meerkat-mobkit --bin rpc_gateway"
    ),
)


_MOB_TOML = """\
[mob]
id = "profile-capabilities-python-smoke"

[profiles.domain]
model = "gpt-5.5"
system_prompt = "You are a deterministic profile-capability smoke-test profile."
external_addressable = true

[profiles.domain.tools]
comms = true
shell = false

[profiles.security]
model = "gpt-5.5"
system_prompt = "You are a deterministic security smoke-test profile."
external_addressable = true

[profiles.security.tools]
comms = true
shell = true
"""


class MutableRoster:
    def __init__(self, security_profile: str = "domain") -> None:
        self.security_profile = security_profile

    async def roster(self, context: dict) -> list[DurableAgentSpec]:
        return [
            DurableAgentSpec(
                identity="domain:plain",
                profile="domain",
                addressability="addressable",
            ),
            DurableAgentSpec(
                identity="domain:security",
                profile=self.security_profile,
                addressability="addressable",
            ),
        ]


async def _build_runtime(mob_toml: Path, state_dir: Path, roster: MutableRoster):
    return await (
        MobKit.builder()
        .gateway(_GATEWAY_BIN)
        .mob(str(mob_toml))
        .persistent_state(str(state_dir))
        .roster(roster)
        .build()
    )


async def _assert_security_shell_surface(handle) -> None:
    tools = set(await handle.identity_resolved_tools("domain:security"))
    assert "shell" in tools
    assert tools.isdisjoint(_BROAD_BUILTIN_TOOLS)


async def _assert_stale_runtime_id_rejected(
    handle,
    stale_runtime_id: str,
    stale_label: tuple[str, str],
) -> None:
    member_ids = {member.agent_identity for member in await handle.list_members()}
    assert stale_runtime_id not in member_ids

    found = await handle.find_members(stale_label[0], stale_label[1])
    assert stale_runtime_id not in {member.agent_identity for member in found}

    async def assert_stale(call):
        with pytest.raises(RpcError) as exc_info:
            await call()
        assert exc_info.value.data["kind"] == "stale_identity_runtime_binding"
        assert exc_info.value.data["live_runtime_member_id"] == stale_runtime_id

    await assert_stale(lambda: handle.get_member(stale_runtime_id))
    await assert_stale(lambda: handle.member_status(stale_runtime_id))
    await assert_stale(lambda: handle.identity_resolved_tools(stale_runtime_id))
    await assert_stale(lambda: handle.send(stale_runtime_id, "should not deliver"))
    await assert_stale(lambda: handle.retire_member(stale_runtime_id))
    await assert_stale(lambda: handle.respawn_member(stale_runtime_id))
    await assert_stale(lambda: handle.force_cancel_member(stale_runtime_id))


@_skip_no_binary
@pytest.mark.asyncio
@pytest.mark.timeout(90)
async def test_real_gateway_reset_reprofile_materializes_shell_tools(tmp_path):
    mob_toml = tmp_path / "mob.toml"
    mob_toml.write_text(_MOB_TOML)
    state_dir = tmp_path / "state"
    state_dir.mkdir()
    roster = MutableRoster(security_profile="domain")

    runtime = await _build_runtime(mob_toml, state_dir, roster)
    try:
        handle = runtime.mob_handle()

        await runtime.reset("domain:security")
        domain_status = await runtime.status("domain:security")
        assert domain_status.profile == "domain"
        old_runtime_id = domain_status.agent_runtime_id
        assert old_runtime_id
        old_member = await handle.get_member(old_runtime_id)
        stale_label = next(iter(old_member.labels.items()), None)
        assert stale_label is not None
        assert "shell" not in await handle.identity_resolved_tools("domain:security")

        roster.security_profile = "security"
        await runtime.reset("domain:security")

        security_status = await runtime.status("domain:security")
        assert security_status.profile == "security"
        assert security_status.agent_runtime_id != old_runtime_id
        await _assert_stale_runtime_id_rejected(handle, old_runtime_id, stale_label)
        await _assert_security_shell_surface(handle)

        await runtime.shutdown()
        runtime = await _build_runtime(mob_toml, state_dir, roster)
        handle = runtime.mob_handle()
        resumed_status = await runtime.status("domain:security")
        assert resumed_status.profile == "security"
        await _assert_security_shell_surface(handle)

        await runtime.reset("domain:plain")
        assert "shell" not in await handle.identity_resolved_tools("domain:plain")
    finally:
        await runtime.shutdown()
