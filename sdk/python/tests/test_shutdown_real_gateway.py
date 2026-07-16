"""Real SDK/gateway teardown regression for callback-backed lease providers."""
from __future__ import annotations

import os

import pytest

from meerkat_mobkit.builder import MobKit
from meerkat_mobkit.identity_first_models import (
    DurableAgentSpec,
    IdentityBootstrapMode,
)
from meerkat_mobkit.identity_first_providers import (
    ContinuityResolveState,
    LeaseAcquireResult,
    LeaseGrant,
    LeaseRenewResult,
)


def _resolve_gateway_bin() -> str:
    override = os.environ.get("MOBKIT_GATEWAY_BIN", "").strip()
    if override:
        return override
    return os.path.join(
        os.path.expanduser("~/Library/Caches/rust-workspaces"),
        "meerkat-mobkit-2783c42580",
        "targets",
        "meerkat-mobkit-44eecf13a1",
        "debug",
        "rpc_gateway",
    )


_GATEWAY_BIN = _resolve_gateway_bin()
_IDENTITY = "agent:shutdown"
_EXACT_GRANT = LeaseGrant(
    identity=_IDENTITY,
    fencing_token=991,
    ttl_ms=600_000,
)
_MOB_TOML = """\
[mob]
id = "shutdown-real-gateway"

[profiles.default]
model = "gpt-5.5"
external_addressable = true

[profiles.default.tools]
comms = true
"""


class _Roster:
    async def roster(self, context):
        del context
        return [DurableAgentSpec(identity=_IDENTITY, profile="default")]


class _ContinuityStore:
    async def resolve_many(self, identities):
        return {
            identity: ContinuityResolveState(state="uninitialized")
            for identity in identities
        }

    async def load_session_snapshot(self, session_id):
        del session_id
        return None

    async def save_session_snapshot(self, *args):
        del args

    async def upsert_continuity_record(self, *args):
        del args

    async def delete_continuity_record(self, *args):
        del args

    async def delete_session_snapshot_if_current_revision(self, *args):
        del args
        return False


class _ExactLeaseProvider:
    def __init__(self):
        self.released: list[LeaseGrant] = []
        self.gateway_process = None

    async def acquire_leases(self, identities, runtime_instance):
        del runtime_instance
        assert identities == [_IDENTITY]
        return {
            _IDENTITY: LeaseAcquireResult(status="acquired", grant=_EXACT_GRANT)
        }

    async def renew_leases(self, grants):
        return {
            grant.identity: LeaseRenewResult(status="renewed", grant=grant)
            for grant in grants
        }

    async def release_leases(self, grants):
        assert self.gateway_process is not None
        assert self.gateway_process.poll() is None, (
            "release callback arrived only after the gateway process exited"
        )
        self.released.extend(grants)


@pytest.mark.skipif(
    not os.path.isfile(_GATEWAY_BIN),
    reason=f"Gateway binary not found at {_GATEWAY_BIN}",
)
@pytest.mark.asyncio
@pytest.mark.timeout(90)
async def test_runtime_shutdown_releases_exact_external_grant_before_process_exit(
    tmp_path,
):
    scratch_dir = tmp_path / "scratch"
    scratch_dir.mkdir()
    lease_provider = _ExactLeaseProvider()
    runtime = await (
        MobKit.builder()
        .gateway(_GATEWAY_BIN)
        .mob_inline(_MOB_TOML)
        .roster(_Roster())
        .continuity_store(_ContinuityStore())
        .lease_provider(lease_provider)
        .scratch_dir(str(scratch_dir))
        .identity_bootstrap_mode(IdentityBootstrapMode.eager_materialize())
        .build()
    )

    transport = runtime._transport
    assert transport is not None
    process = transport._process
    assert process is not None and process.poll() is None
    lease_provider.gateway_process = process

    await runtime.shutdown()

    assert lease_provider.released == [_EXACT_GRANT]
    assert process.poll() is not None
