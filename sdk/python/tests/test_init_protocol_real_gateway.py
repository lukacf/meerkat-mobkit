"""Real-gateway counterpart of ``test_init_protocol.py`` (#550, part 3).

A startup provider callback that issues an ordinary RPC on the runtime that
is starting gets the typed ``InitInProgressError`` at once, instead of
queueing behind init and waiting on itself. Init then settles ready and the
same RPC is served.
"""
from __future__ import annotations

import os
import time

import pytest

from meerkat_mobkit import InitInProgressError
from meerkat_mobkit.builder import MobKit
from meerkat_mobkit.runtime import MobKitRuntime

_GATEWAY_BIN = os.environ.get("MOBKIT_GATEWAY_BIN", "").strip()

_MOB_TOML = """\
[mob]
id = "init-protocol-real-gateway"

[profiles.default]
model = "gpt-5.5"
external_addressable = true
"""


class _ReentrantRoster:
    """Calls back into the starting runtime from inside the roster callback."""

    def __init__(self):
        self.runtime: MobKitRuntime | None = None
        self.refusals: list[tuple[InitInProgressError, float]] = []

    async def roster(self, context):
        del context
        assert self.runtime is not None
        started = time.monotonic()
        try:
            await self.runtime._rpc("mobkit/status")
        except InitInProgressError as refusal:
            self.refusals.append((refusal, time.monotonic() - started))
        return []


@pytest.mark.skipif(
    not (_GATEWAY_BIN and os.path.isfile(_GATEWAY_BIN)),
    reason="MOBKIT_GATEWAY_BIN not set to a built rpc_gateway",
)
@pytest.mark.asyncio
async def test_reentrant_rpc_from_a_startup_callback_is_refused_at_once():
    roster = _ReentrantRoster()
    runtime = MobKitRuntime(
        MobKit.builder().gateway(_GATEWAY_BIN).mob_inline(_MOB_TOML).roster(roster)._config
    )
    roster.runtime = runtime
    try:
        await runtime.connect()
        assert runtime.is_running
        assert roster.refusals, "the roster callback ran during init"
        for refusal, elapsed in roster.refusals:
            assert refusal.data["kind"] == "init_in_progress"
            assert refusal.data["method"] == "mobkit/status"
            # Refused at once, not after queueing behind init: far inside the
            # transport's request timeout.
            assert elapsed < 10, elapsed
        # After init settled, the same request is served.
        status = await runtime._rpc("mobkit/status")
        assert isinstance(status, dict)
    finally:
        await runtime.shutdown()
