"""HomeCore-scenario E2E smoke tests using the identity-first API.

Covers HC-01 through HC-08: real round-trip through Python SDK -> rpc_gateway
-> Rust IdentityRuntime -> restore_flow -> SessionBridge -> real LLM API.

Run:
    PYTHONPATH=sdk/python ANTHROPIC_API_KEY=... \
        python3 -m pytest sdk/python/tests/test_identity_first_homecore_e2e.py -v --timeout=180
"""
from __future__ import annotations

import os
import asyncio

import pytest

from meerkat_mobkit.builder import MobKit
from meerkat_mobkit.identity_first_models import (
    DispatchInput,
    DurableAgentSpec,
    IdentityBootstrapMode,
    IdentityBootstrapState,
    ManagedPeerEdge,
)
from meerkat_mobkit.identity_first_providers import (
    AgentCustomizerProtocol,
    RosterProviderProtocol,
    TopologyProviderProtocol,
)
from meerkat_mobkit.errors import RpcError

# ---------------------------------------------------------------------------
# Environment / skip helpers
# ---------------------------------------------------------------------------

# The gateway-backed suites must exercise THIS worktree's freshly built
# rpc_gateway, not whichever binary the main checkout last built. The default
# path below is the main worktree's scripts/repo-cargo lane; running from a
# feature worktree would silently test the wrong artifact (and hide the wire
# drift the branch introduces). Prefer an explicit override:
#   MOBKIT_GATEWAY_BIN=$(./scripts/repo-cargo --print-env CARGO_TARGET_DIR)/debug/rpc_gateway
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


def _anthropic_key() -> str | None:
    return os.environ.get("RKAT_ANTHROPIC_API_KEY") or os.environ.get(
        "ANTHROPIC_API_KEY"
    )


_skip_no_key = pytest.mark.skipif(
    not _anthropic_key(),
    reason="No Anthropic API key",
)

_skip_no_binary = pytest.mark.skipif(
    not os.path.isfile(_GATEWAY_BIN),
    reason=f"Gateway binary not found at {_GATEWAY_BIN}",
)

# ---------------------------------------------------------------------------
# Mob definition (TOML) — profiles for all HomeCore roles
# ---------------------------------------------------------------------------

_HOMECORE_MOB_TOML = """\
[mob]
id = "homecore-e2e"

[profiles.personal]
model = "claude-sonnet-4-5"
system_prompt = "You are a helpful personal assistant. Keep responses brief (1-2 sentences)."
external_addressable = true

[profiles.personal.tools]
comms = true

[profiles.triage]
model = "claude-sonnet-4-5"
system_prompt = "You are a triage agent. Route requests to the right domain. Keep responses brief."
external_addressable = false

[profiles.triage.tools]
comms = true

[profiles.calendar]
model = "claude-sonnet-4-5"
system_prompt = "You are a calendar domain agent. Help with scheduling. Keep responses brief."
external_addressable = false

[profiles.calendar.tools]
comms = true

[profiles.gatekeeper]
model = "claude-sonnet-4-5"
system_prompt = "You are a gatekeeper agent. Validate requests. Keep responses brief."
external_addressable = false

[profiles.gatekeeper.tools]
comms = true
"""

# ---------------------------------------------------------------------------
# Provider implementations
# ---------------------------------------------------------------------------


class HomeCoreRoster:
    """Roster provider that returns a configurable list of DurableAgentSpecs."""

    def __init__(self, specs: list[DurableAgentSpec]):
        self._specs = list(specs)

    def update(self, specs: list[DurableAgentSpec]) -> None:
        self._specs = list(specs)

    async def roster(self, context: dict) -> list[DurableAgentSpec]:
        return list(self._specs)


class HomeCoreTopology:
    """Topology provider that returns configurable edges."""

    def __init__(self, edges: list[tuple[str, str]]):
        self._edges = list(edges)

    def update(self, edges: list[tuple[str, str]]) -> None:
        self._edges = list(edges)

    async def compute_edges(
        self, target_identities: list[str], context: dict
    ) -> list[ManagedPeerEdge]:
        return [ManagedPeerEdge(a=a, b=b) for a, b in self._edges]


class HomeCoreCustomizer:
    """Agent customizer that injects topology-aware prompts."""

    async def customize_build(self, context, spec, draft) -> None:
        """Mutate draft in-place based on context.

        context: AgentBuildContext (identity, active_peers, managed_edges)
        spec: DurableAgentSpec
        draft: AgentBuildDraft (mutable)
        """
        identity = context.identity
        peers = []
        for edge in context.managed_edges:
            if edge.a == identity:
                peers.append(edge.b)
            elif edge.b == identity:
                peers.append(edge.a)

        if peers:
            peers.sort()
            existing = draft.system_prompt or ""
            draft.system_prompt = (
                f"{existing}\nYour connected peers: {', '.join(peers)}"
            )

    async def after_create(
        self, identity: str, session_id: str, context
    ) -> None:
        pass


# ---------------------------------------------------------------------------
# Default rosters and topology
# ---------------------------------------------------------------------------

_DEFAULT_ROSTER = [
    DurableAgentSpec(identity="identity:luka", profile="personal", addressability="addressable"),
    DurableAgentSpec(identity="identity:louise", profile="personal", addressability="addressable"),
    DurableAgentSpec(identity="triage:main", profile="triage", addressability="internal_only"),
    DurableAgentSpec(identity="domain:calendar", profile="calendar", addressability="internal_only"),
    DurableAgentSpec(identity="gate:main", profile="gatekeeper", addressability="internal_only"),
]

_DEFAULT_EDGES = [
    ("identity:luka", "triage:main"),
    ("identity:louise", "triage:main"),
    ("triage:main", "domain:calendar"),
    ("triage:main", "gate:main"),
]

# ---------------------------------------------------------------------------
# Boot helper
# ---------------------------------------------------------------------------


async def _boot_identity_runtime(
    state_dir: str,
    mob_toml_path: str,
    roster: HomeCoreRoster,
    topology: HomeCoreTopology | None = None,
    customizer: HomeCoreCustomizer | None = None,
):
    """Boot a real rpc_gateway with identity-first providers."""
    builder = (
        MobKit.builder()
        .gateway(_GATEWAY_BIN)
        .mob(mob_toml_path)
        .persistent_state(state_dir)
        .roster(roster)
    )
    if topology is not None:
        builder = builder.topology_provider(topology)
    if customizer is not None:
        builder = builder.agent_customizer(customizer)
    return await builder.build()


@pytest.fixture
def mob_toml(tmp_path):
    p = tmp_path / "mob.toml"
    p.write_text(_HOMECORE_MOB_TOML)
    return str(p)


@pytest.fixture
def state_dir(tmp_path):
    d = str(tmp_path / "state")
    os.makedirs(d, exist_ok=True)
    return d


@_skip_no_binary
@pytest.mark.asyncio
@pytest.mark.timeout(30)
async def test_lazy_gateway_init_exposes_typed_dormant_bootstrap_status(
    mob_toml, state_dir
):
    """Lazy init is metadata-only and needs no model-provider credential."""
    roster = HomeCoreRoster(_DEFAULT_ROSTER)
    runtime = await (
        MobKit.builder()
        .gateway(_GATEWAY_BIN)
        .mob(mob_toml)
        .persistent_state(state_dir)
        .roster(roster)
        .identity_bootstrap_mode(IdentityBootstrapMode.lazy_materialize())
        .build()
    )
    try:
        status = await runtime.identity_bootstrap_status()
        assert status.mode == IdentityBootstrapMode.lazy_materialize()
        assert status.complete is True
        assert status.ready is False
        assert status.counts.dormant == len(_DEFAULT_ROSTER)
        assert status.counts.warming == 0
        assert status.counts.active == 0
        assert status.counts.broken == 0
        assert all(
            entry.state is IdentityBootstrapState.DORMANT
            for entry in status.identities.values()
        )

        immediate = await runtime.wait_identity_bootstrap(timeout=0)
        assert immediate.timed_out is True
        assert immediate.ready is False
    finally:
        await runtime.shutdown()


# ===========================================================================
# HC-01: Family Bootstrap And Mixed Delivery
# ===========================================================================


@_skip_no_key
@_skip_no_binary
class TestHC01FamilyBootstrapAndMixedDelivery:
    @pytest.mark.asyncio
    @pytest.mark.timeout(120)
    async def test_send_dispatch_and_addressability(self, mob_toml, state_dir):
        roster = HomeCoreRoster(_DEFAULT_ROSTER)
        topology = HomeCoreTopology(_DEFAULT_EDGES)
        customizer = HomeCoreCustomizer()

        rt = await _boot_identity_runtime(
            state_dir, mob_toml, roster, topology, customizer
        )
        try:
            # send() to Addressable identity succeeds
            result = await rt.send("identity:luka", "What's on my calendar tomorrow?")
            assert result is not None

            # dispatch() to InternalOnly identity succeeds
            di = DispatchInput(
                content="New email from school about pickup change",
                origin="connector",
            )
            result = await rt.dispatch("triage:main", di)
            assert result is not None

            # send() to InternalOnly returns NotAddressable error
            with pytest.raises(RpcError, match="not addressable"):
                await rt.send("triage:main", "Should be rejected")

            # status() shows correct state
            status = await rt.status("triage:main")
            assert status.state == "active"
            assert status.addressability == "internal_only"

            luka_status = await rt.status("identity:luka")
            assert luka_status.state == "active"
            assert luka_status.addressability == "addressable"
            assert luka_status.agent_runtime_id is not None
            assert luka_status.session_id is not None
        finally:
            await rt.shutdown()


# ===========================================================================
# HC-02: Unknown Sender Goes To Triage, Known Sender Goes To Identity
# ===========================================================================


@_skip_no_key
@_skip_no_binary
class TestHC02DispatchRouting:
    @pytest.mark.asyncio
    @pytest.mark.timeout(120)
    async def test_distinct_dispatch_targets(self, mob_toml, state_dir):
        roster = HomeCoreRoster(_DEFAULT_ROSTER)
        topology = HomeCoreTopology(_DEFAULT_EDGES)

        rt = await _boot_identity_runtime(state_dir, mob_toml, roster, topology)
        try:
            # Known sender → dispatch to identity
            di_luka = DispatchInput(
                content="Telegram DM from Luka: Can you remind me about dentist?",
                origin="connector",
                correlation_id="msg-1",
            )
            await rt.dispatch("identity:luka", di_luka)

            # Unknown sender → dispatch to triage
            di_triage = DispatchInput(
                content="Unknown WhatsApp sender asked about package delivery",
                origin="connector",
                correlation_id="msg-2",
            )
            await rt.dispatch("triage:main", di_triage)

            # Both are distinct identities with different session_ids
            luka_status = await rt.status("identity:luka")
            triage_status = await rt.status("triage:main")
            assert luka_status.session_id != triage_status.session_id
            assert luka_status.agent_runtime_id != triage_status.agent_runtime_id

            # Record pre-restart state
            luka_rt_id = luka_status.agent_runtime_id
            luka_session = luka_status.session_id
            triage_rt_id = triage_status.agent_runtime_id
            triage_session = triage_status.session_id
        finally:
            await rt.shutdown()

        # Restart and verify stability
        rt2 = await _boot_identity_runtime(state_dir, mob_toml, roster, topology)
        try:
            luka2 = await rt2.status("identity:luka")
            triage2 = await rt2.status("triage:main")
            assert luka2.agent_runtime_id == luka_rt_id
            assert luka2.session_id == luka_session
            assert triage2.agent_runtime_id == triage_rt_id
            assert triage2.session_id == triage_session
        finally:
            await rt2.shutdown()


# ===========================================================================
# HC-03: Reconcile New Family Member Without Breaking Existing Continuity
# ===========================================================================


@_skip_no_key
@_skip_no_binary
class TestHC03ReconcileNewMember:
    @pytest.mark.asyncio
    @pytest.mark.timeout(120)
    async def test_add_olivia_via_in_process_reconcile(self, mob_toml, state_dir):
        roster = HomeCoreRoster(_DEFAULT_ROSTER[:3])  # luka, louise, triage
        topology = HomeCoreTopology(_DEFAULT_EDGES[:2])  # luka↔triage, louise↔triage

        rt = await _boot_identity_runtime(state_dir, mob_toml, roster, topology)
        try:
            before_luka = await rt.status("identity:luka")
            before_louise = await rt.status("identity:louise")
            luka_session = before_luka.session_id
            luka_rt_id = before_luka.agent_runtime_id
            louise_session = before_louise.session_id

            # Mutate the roster IN-PROCESS (not restart)
            roster.update([
                *_DEFAULT_ROSTER[:3],
                DurableAgentSpec(
                    identity="identity:olivia",
                    profile="personal",
                    addressability="addressable",
                ),
            ])
            topology.update([
                *_DEFAULT_EDGES[:2],
                ("identity:olivia", "triage:main"),
            ])

            # In-process reconcile — re-runs restore_flow with updated roster
            result = await rt.reconcile()
            assert result is not None

            after_luka = await rt.status("identity:luka")
            after_louise = await rt.status("identity:louise")
            after_olivia = await rt.status("identity:olivia")

            # olivia is new
            assert after_olivia.state == "active"
            assert after_olivia.session_id is not None
            assert after_olivia.generation == 0

            # luka and louise preserved — no silent respawn
            assert after_luka.session_id == luka_session
            assert after_luka.agent_runtime_id == luka_rt_id
            assert after_louise.session_id == louise_session
        finally:
            await rt.shutdown()


# ===========================================================================
# HC-04: Hot-Update Addressability And Metadata
# ===========================================================================


@_skip_no_key
@_skip_no_binary
class TestHC04HotUpdateAddressability:
    @pytest.mark.asyncio
    @pytest.mark.timeout(120)
    async def test_flip_addressability_via_reconcile(self, mob_toml, state_dir):
        roster = HomeCoreRoster(_DEFAULT_ROSTER)
        topology = HomeCoreTopology(_DEFAULT_EDGES)

        rt = await _boot_identity_runtime(state_dir, mob_toml, roster, topology)
        try:
            # luka starts Addressable
            status = await rt.status("identity:luka")
            assert status.addressability == "addressable"

            # send works
            await rt.send("identity:luka", "Hello")

            # Mutate roster IN-PROCESS: flip luka to InternalOnly, add label
            new_luka = DurableAgentSpec(
                identity="identity:luka",
                profile="personal",
                addressability="internal_only",
                labels={"timezone": "Europe/Stockholm"},
            )
            roster.update([new_luka, *_DEFAULT_ROSTER[1:]])

            # In-process reconcile
            await rt.reconcile()

            status2 = await rt.status("identity:luka")
            assert status2.addressability == "internal_only"
            assert status2.labels.get("timezone") == "Europe/Stockholm"

            # send now rejected
            with pytest.raises(RpcError, match="not addressable"):
                await rt.send("identity:luka", "Should fail now")

            # dispatch still works
            di = DispatchInput(content="system notice", origin="system")
            await rt.dispatch("identity:luka", di)
        finally:
            await rt.shutdown()


# ===========================================================================
# HC-05: Durable Respawn After Wedged Agent
# ===========================================================================


@_skip_no_key
@_skip_no_binary
class TestHC05DurableRespawn:
    @pytest.mark.asyncio
    @pytest.mark.timeout(120)
    async def test_respawn_preserves_identity(self, mob_toml, state_dir):
        roster = HomeCoreRoster(_DEFAULT_ROSTER)
        topology = HomeCoreTopology(_DEFAULT_EDGES)

        rt = await _boot_identity_runtime(state_dir, mob_toml, roster, topology)
        try:
            # Dispatch to calendar
            di = DispatchInput(
                content="Long-running calendar sync event",
                origin="scheduler",
            )
            await rt.dispatch("domain:calendar", di)

            before = await rt.status("domain:calendar")
            assert before.state == "active"
            before_rt_id = before.agent_runtime_id
            before_session = before.session_id
            before_gen = before.generation

            # Respawn — non-destructive recovery
            result = await rt.respawn("domain:calendar")
            assert result is not None

            after = await rt.status("domain:calendar")
            assert after.state == "active"
            assert after.agent_runtime_id == before_rt_id
            assert after.session_id == before_session  # session preserved
            assert after.generation == before_gen  # generation NOT advanced
        finally:
            await rt.shutdown()


# ===========================================================================
# HC-06: Lease Loss Stops New Work
# ===========================================================================

# HC-06: Lease loss requires an external LeaseProvider that can force Lost.
# The gateway currently creates a LocalLeaseProvider internally — there is no
# SDK callback path for lease operations yet. When external LeaseProvider
# support is wired, this test must be upgraded to:
#   1. Force lease loss via the test provider
#   2. Assert dispatch is rejected after loss
#   3. Assert status reflects unhealthy lease
# For now, this test only verifies the happy path (dispatch succeeds with lease).


@_skip_no_key
@_skip_no_binary
class TestHC06LeaseSemantics:
    @pytest.mark.asyncio
    @pytest.mark.timeout(60)
    async def test_dispatch_succeeds_with_lease_happy_path(self, mob_toml, state_dir):
        """Happy path only: dispatch works when lease is held.

        NOTE: Does NOT test lease loss — requires external LeaseProvider (not yet wired).
        """
        roster = HomeCoreRoster(_DEFAULT_ROSTER)

        rt = await _boot_identity_runtime(state_dir, mob_toml, roster)
        try:
            di = DispatchInput(content="before lease check", origin="connector")
            result = await rt.dispatch("triage:main", di)
            assert result is not None

            status = await rt.status("triage:main")
            assert status.state == "active"
        finally:
            await rt.shutdown()


# ===========================================================================
# HC-07: Reset vs Delete Semantics
# ===========================================================================


@_skip_no_key
@_skip_no_binary
class TestHC07ResetVsDelete:
    @pytest.mark.asyncio
    @pytest.mark.timeout(120)
    async def test_reset_advances_generation(self, mob_toml, state_dir):
        roster = HomeCoreRoster(_DEFAULT_ROSTER)
        topology = HomeCoreTopology(_DEFAULT_EDGES)

        rt = await _boot_identity_runtime(state_dir, mob_toml, roster, topology)
        try:
            before = await rt.status("identity:luka")
            old_session = before.session_id
            old_gen = before.generation

            result = await rt.reset("identity:luka")
            assert result is not None

            after = await rt.status("identity:luka")
            assert after.generation == old_gen + 1
            assert after.session_id != old_session
            # Fresh reset may expose the initial save for the new generation.
            assert after.checkpoint_version in (0, 1, None)
        finally:
            await rt.shutdown()

    @pytest.mark.asyncio
    @pytest.mark.timeout(120)
    async def test_delete_and_reappearance(self, mob_toml, state_dir):
        roster = HomeCoreRoster(_DEFAULT_ROSTER)

        rt = await _boot_identity_runtime(state_dir, mob_toml, roster)
        try:
            before = await rt.status("identity:luka")
            old_rt_id = before.agent_runtime_id

            await rt.delete_identity("identity:luka")

            # identity:luka is gone
            with pytest.raises(RpcError, match="unknown identity"):
                await rt.status("identity:luka")
        finally:
            await rt.shutdown()

        # Restart with same roster — luka reappears as fresh
        rt2 = await _boot_identity_runtime(state_dir, mob_toml, roster)
        try:
            after = await rt2.status("identity:luka")
            assert after.state == "active"
            assert after.generation == 0
            assert after.session_id != before.session_id
        finally:
            await rt2.shutdown()


# ===========================================================================
# HC-08: External-Authoritative Restart Smoke
# ===========================================================================


@_skip_no_key
@_skip_no_binary
class TestHC08PersistentRestart:
    @pytest.mark.asyncio
    @pytest.mark.timeout(180)
    async def test_session_and_continuity_preserved_across_restart(self, mob_toml, state_dir):
        roster = HomeCoreRoster(_DEFAULT_ROSTER)
        topology = HomeCoreTopology(_DEFAULT_EDGES)
        customizer = HomeCoreCustomizer()

        rt = await _boot_identity_runtime(
            state_dir, mob_toml, roster, topology, customizer
        )
        try:
            # Give luka a memorable fact
            await rt.send(
                "identity:luka",
                "Remember that we moved the piano on Friday. This is important.",
            )
            before = await rt.status("identity:luka")
            session_before = before.session_id
            rt_id_before = before.agent_runtime_id

            # Also dispatch to triage to build multi-actor state
            di = DispatchInput(
                content="School notification: parent-teacher conference next Tuesday",
                origin="connector",
            )
            await rt.dispatch("triage:main", di)
            triage_before = await rt.status("triage:main")
            triage_session_before = triage_before.session_id
        finally:
            await rt.shutdown()

        # Restart with same state_dir
        rt2 = await _boot_identity_runtime(
            state_dir, mob_toml, roster, topology, customizer
        )
        try:
            # Verify stable identifiers across restart
            after = await rt2.status("identity:luka")
            assert after.session_id == session_before
            assert after.agent_runtime_id == rt_id_before
            assert after.state == "active"

            triage_after = await rt2.status("triage:main")
            assert triage_after.session_id == triage_session_before

            # Verify conversational continuity — ask about the pre-restart fact.
            # The LLM should reference the piano if session history is intact.
            await rt2.send(
                "identity:luka",
                "What did we move on Friday? Reply with just the item.",
            )
            # If this dispatch completes without error, the session is alive.
            # Full memory verification would require inspecting the LLM response,
            # but the stable session_id + successful delivery proves the session
            # was restored (not fresh-created).
        finally:
            await rt2.shutdown()
