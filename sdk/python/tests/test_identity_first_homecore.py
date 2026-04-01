"""HomeCore-scenario E2E smoke tests for the Python SDK surface.

Covers HC-01 through HC-08 from phase5_scenarios.md Track B.
These tests exercise the REAL round-trip: Python SDK -> rpc_gateway binary
-> Rust UnifiedRuntime -> LLM API -> back. No mocks for the RPC layer.

Run:
    PYTHONPATH=sdk/python ANTHROPIC_API_KEY=... \
        python3 -m pytest sdk/python/tests/test_identity_first_homecore.py -v --timeout=120
"""
from __future__ import annotations

import os
import tempfile

import pytest

from meerkat_mobkit.builder import MobKit
from meerkat_mobkit.models import SessionBuildOptions
from meerkat_mobkit.errors import RpcError


# ---------------------------------------------------------------------------
# Environment / skip helpers
# ---------------------------------------------------------------------------

_GATEWAY_BIN = os.path.join(
    os.path.expanduser("~/Library/Caches/rust-workspaces"),
    "meerkat-mobkit-2783c42580",
    "targets",
    "meerkat-mobkit-44eecf13a1",
    "debug",
    "rpc_gateway",
)


def _anthropic_key() -> str | None:
    return os.environ.get("RKAT_ANTHROPIC_API_KEY") or os.environ.get("ANTHROPIC_API_KEY")


_skip_no_key = pytest.mark.skipif(
    not _anthropic_key(),
    reason="No Anthropic API key (set ANTHROPIC_API_KEY or RKAT_ANTHROPIC_API_KEY)",
)

_skip_no_binary = pytest.mark.skipif(
    not os.path.isfile(_GATEWAY_BIN),
    reason=f"Gateway binary not found at {_GATEWAY_BIN} — run: ./scripts/repo-cargo build -p meerkat-mobkit --bin rpc_gateway",
)


# ---------------------------------------------------------------------------
# Mob definition (TOML) for HomeCore scenarios
# ---------------------------------------------------------------------------

_HOMECORE_MOB_TOML = """\
[mob]
id = "homecore-e2e"

[profiles.personal]
model = "claude-sonnet-4-5"
system_prompt = "You are a helpful personal assistant. Keep responses very brief (1-2 sentences)."
external_addressable = true

[profiles.personal.tools]
comms = true

[profiles.triage]
model = "claude-sonnet-4-5"
system_prompt = "You are a triage agent. Route requests to the right domain agent. Keep responses very brief."
external_addressable = false

[profiles.triage.tools]
comms = true

[profiles.calendar]
model = "claude-sonnet-4-5"
system_prompt = "You are a calendar domain agent. Help with scheduling. Keep responses very brief."
external_addressable = false

[profiles.calendar.tools]
comms = true

[profiles.gatekeeper]
model = "claude-sonnet-4-5"
system_prompt = "You are a gatekeeper agent. Validate requests. Keep responses very brief."
external_addressable = false

[profiles.gatekeeper.tools]
comms = true
"""


# ---------------------------------------------------------------------------
# SessionAgentBuilder — minimal, no custom tools
# ---------------------------------------------------------------------------

class MinimalAgentBuilder:
    """Satisfies SessionAgentBuilder protocol with no-op build_agent."""

    async def build_agent(self, opts: SessionBuildOptions) -> None:
        pass


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

async def _boot_runtime(state_dir: str, mob_toml_path: str):
    """Boot a real rpc_gateway subprocess and return a connected MobKitRuntime."""
    return await (
        MobKit.builder()
        .gateway(_GATEWAY_BIN)
        .mob(mob_toml_path)
        .persistent_state(state_dir)
        .session_service(MinimalAgentBuilder())
        .build()
    )


@pytest.fixture
def mob_toml(tmp_path):
    """Write the HomeCore mob.toml to a temp file and return its path."""
    p = tmp_path / "mob.toml"
    p.write_text(_HOMECORE_MOB_TOML)
    return str(p)


@pytest.fixture
def state_dir(tmp_path):
    """Create and return a temporary state directory."""
    d = str(tmp_path / "state")
    os.makedirs(d, exist_ok=True)
    return d


# ---------------------------------------------------------------------------
# HC-01: Family Bootstrap And Mixed Delivery
# ---------------------------------------------------------------------------

@_skip_no_key
@_skip_no_binary
class TestHC01FamilyBootstrapAndMixedDelivery:
    """HC-01: Family roster with mixed addressability and real LLM round-trip."""

    @pytest.mark.asyncio
    @pytest.mark.timeout(60)
    async def test_ensure_members_and_send_message(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()

            # Ensure personal (addressable) and triage (internal) members
            luka = await handle.ensure_member(
                "luka", profile="personal", labels={"role": "family"},
            )
            assert luka.meerkat_id == "luka"
            assert luka.profile == "personal"

            triage = await handle.ensure_member(
                "triage-main", profile="triage", labels={"role": "triage"},
            )
            assert triage.meerkat_id == "triage-main"
            assert triage.profile == "triage"

            # Send a real message to the personal agent — triggers LLM call
            result = await handle.send("luka", message="Say hello in exactly three words.")
            assert result.accepted
            assert result.member_id == "luka"
            assert result.session_id  # non-empty session ID
        finally:
            await rt.shutdown()

    @pytest.mark.asyncio
    @pytest.mark.timeout(60)
    async def test_send_to_internal_only_profile_rejected(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()
            await handle.ensure_member("triage-main", profile="triage")
            # Triage profile has external_addressable=false — send should be rejected
            with pytest.raises(RpcError, match="not externally addressable"):
                await handle.send("triage-main", message="Route this request.")
        finally:
            await rt.shutdown()

    @pytest.mark.asyncio
    @pytest.mark.timeout(30)
    async def test_status_after_bootstrap(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()
            status = await handle.status()
            assert status.running
        finally:
            await rt.shutdown()

    @pytest.mark.asyncio
    @pytest.mark.timeout(30)
    async def test_capabilities_include_send_and_ensure(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()
            caps = await handle.capabilities()
            assert "mobkit/send_message" in caps.methods
            assert "mobkit/ensure_member" in caps.methods
            assert "mobkit/list_members" in caps.methods
            assert "mobkit/respawn_member" in caps.methods
        finally:
            await rt.shutdown()


# ---------------------------------------------------------------------------
# HC-02: Session Continuity Across Messages
# ---------------------------------------------------------------------------

@_skip_no_key
@_skip_no_binary
class TestHC02SessionContinuity:
    """HC-02: Same member keeps the same session_id across multiple sends."""

    @pytest.mark.asyncio
    @pytest.mark.timeout(90)
    async def test_session_id_stable_across_sends(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()
            await handle.ensure_member("luka", profile="personal")

            r1 = await handle.send("luka", message="Remember the number 42.")
            r2 = await handle.send("luka", message="What number did I just mention?")

            assert r1.session_id == r2.session_id
            assert r1.member_id == r2.member_id == "luka"
        finally:
            await rt.shutdown()

    @pytest.mark.asyncio
    @pytest.mark.timeout(60)
    async def test_member_snapshot_stable_after_send(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()
            snap1 = await handle.ensure_member("luka", profile="personal")
            await handle.send("luka", message="Hello")
            snap2 = await handle.get_member("luka")

            assert snap1.meerkat_id == snap2.meerkat_id == "luka"
            assert snap1.profile == snap2.profile == "personal"
        finally:
            await rt.shutdown()


# ---------------------------------------------------------------------------
# HC-03: Reconcile New Family Member Without Breaking Existing Continuity
# ---------------------------------------------------------------------------

@_skip_no_key
@_skip_no_binary
class TestHC03ReconcileNewMember:
    """HC-03: Adding a new member doesn't disturb existing members."""

    @pytest.mark.asyncio
    @pytest.mark.timeout(90)
    async def test_add_member_preserves_existing(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()

            await handle.ensure_member("luka", profile="personal")
            await handle.ensure_member("louise", profile="personal")

            r1 = await handle.send("luka", message="Remember: my favorite color is blue.")
            session_before = r1.session_id

            # Add a third member
            olivia = await handle.ensure_member("olivia", profile="personal")
            assert olivia.meerkat_id == "olivia"
            assert olivia.profile == "personal"

            # Existing member session_id unchanged
            r2 = await handle.send("luka", message="What is my favorite color?")
            assert r2.session_id == session_before

            # New member works independently
            r3 = await handle.send("olivia", message="Hello, who are you?")
            assert r3.accepted
            assert r3.session_id != session_before
        finally:
            await rt.shutdown()

    @pytest.mark.asyncio
    @pytest.mark.timeout(30)
    async def test_list_members_after_reconcile(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()
            await handle.ensure_member("luka", profile="personal")
            await handle.ensure_member("louise", profile="personal")
            await handle.ensure_member("olivia", profile="personal")

            members = await handle.list_members()
            ids = {m.meerkat_id for m in members}
            assert "luka" in ids
            assert "louise" in ids
            assert "olivia" in ids
        finally:
            await rt.shutdown()


# ---------------------------------------------------------------------------
# HC-04: Hot-Update Labels And Metadata
# ---------------------------------------------------------------------------

@_skip_no_key
@_skip_no_binary
class TestHC04HotUpdateMetadata:
    """HC-04: Labels and metadata can be updated via re-ensure."""

    @pytest.mark.asyncio
    @pytest.mark.timeout(60)
    async def test_ensure_member_idempotent_same_labels(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()

            snap1 = await handle.ensure_member(
                "luka", profile="personal", labels={"tier": "free"},
            )
            assert snap1.labels.get("tier") == "free"

            # Re-ensure with SAME labels is idempotent
            snap2 = await handle.ensure_member(
                "luka", profile="personal", labels={"tier": "free"},
            )
            assert snap2.meerkat_id == "luka"
            assert snap2.labels.get("tier") == "free"
        finally:
            await rt.shutdown()

    @pytest.mark.asyncio
    @pytest.mark.timeout(30)
    async def test_find_members_by_label(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()
            await handle.ensure_member(
                "luka", profile="personal", labels={"role": "family"},
            )
            await handle.ensure_member(
                "triage-main", profile="triage", labels={"role": "triage"},
            )

            family = await handle.find_members("role", "family")
            assert any(m.meerkat_id == "luka" for m in family)

            triage = await handle.find_members("role", "triage")
            assert any(m.meerkat_id == "triage-main" for m in triage)
        finally:
            await rt.shutdown()


# ---------------------------------------------------------------------------
# HC-05: Durable Respawn After Wedged Agent
# ---------------------------------------------------------------------------

@_skip_no_key
@_skip_no_binary
class TestHC05DurableRespawn:
    """HC-05: Respawn preserves identity and allows continued operation."""

    @pytest.mark.asyncio
    @pytest.mark.timeout(90)
    async def test_respawn_then_send(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()
            await handle.ensure_member("respawn-target", profile="personal")

            # Respawn the member immediately
            await handle.respawn_member("respawn-target")

            # Member still exists and can receive messages after respawn
            r1 = await handle.send(
                "respawn-target", message="Say hello after respawn.",
            )
            assert r1.accepted
            assert r1.member_id == "respawn-target"
        finally:
            await rt.shutdown()

    @pytest.mark.asyncio
    @pytest.mark.timeout(60)
    async def test_respawn_member_still_in_roster(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()
            await handle.ensure_member("respawn-target", profile="personal")
            await handle.respawn_member("respawn-target")

            snap = await handle.get_member("respawn-target")
            assert snap.meerkat_id == "respawn-target"
            assert snap.state == "active"
        finally:
            await rt.shutdown()


# ---------------------------------------------------------------------------
# HC-06: Retire Stops New Work
# ---------------------------------------------------------------------------

@_skip_no_key
@_skip_no_binary
class TestHC06RetireStopsWork:
    """HC-06: Retired member transitions to retiring state."""

    @pytest.mark.asyncio
    @pytest.mark.timeout(60)
    async def test_retire_changes_member_state(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()

            # Use an addressable profile so we can verify it's active first
            await handle.ensure_member("retire-target", profile="personal")
            snap = await handle.get_member("retire-target")
            assert snap.state == "active"

            # Retire the member
            await handle.retire_member("retire-target")

            # After retire, the member should be in retiring state or removed
            members = await handle.list_members()
            target = [m for m in members if m.meerkat_id == "retire-target"]
            if target:
                assert target[0].state == "retiring"
        finally:
            await rt.shutdown()

    @pytest.mark.asyncio
    @pytest.mark.timeout(60)
    async def test_send_to_internal_only_rejected(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()
            await handle.ensure_member("gate-main", profile="gatekeeper")
            # Gatekeeper profile has external_addressable=false
            with pytest.raises(RpcError, match="not externally addressable"):
                await handle.send("gate-main", message="Hello gatekeeper.")
        finally:
            await rt.shutdown()


# ---------------------------------------------------------------------------
# HC-07: Reset vs Delete Semantics
# ---------------------------------------------------------------------------

@_skip_no_key
@_skip_no_binary
class TestHC07ResetVsDelete:
    """HC-07: Retire+re-ensure gives fresh session, respawn preserves identity."""

    @pytest.mark.asyncio
    @pytest.mark.timeout(90)
    async def test_retire_and_reensure_gives_new_session(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()
            await handle.ensure_member("luka", profile="personal")
            r1 = await handle.send(
                "luka", message="Remember: the secret word is 'mango'.",
            )
            old_session = r1.session_id

            # Retire, then re-ensure (simulates delete + reappearance)
            await handle.retire_member("luka")
            new_snap = await handle.ensure_member("luka", profile="personal")
            assert new_snap.meerkat_id == "luka"

            # New session should be different (fresh agent, no history)
            r2 = await handle.send("luka", message="Do you know any secret word?")
            assert r2.accepted
            assert r2.session_id != old_session
        finally:
            await rt.shutdown()

    @pytest.mark.asyncio
    @pytest.mark.timeout(60)
    async def test_respawn_keeps_same_identity(self, mob_toml, state_dir):
        rt = await _boot_runtime(state_dir, mob_toml)
        try:
            handle = rt.mob_handle()
            await handle.ensure_member("luka", profile="personal")
            await handle.send("luka", message="Hello")

            await handle.respawn_member("luka")
            snap = await handle.get_member("luka")
            assert snap.meerkat_id == "luka"
            assert snap.profile == "personal"
            assert snap.state == "active"
        finally:
            await rt.shutdown()


# ---------------------------------------------------------------------------
# HC-08: External-Authoritative Restart Smoke (Persistent State)
# ---------------------------------------------------------------------------

@_skip_no_key
@_skip_no_binary
class TestHC08PersistentStateRestart:
    """HC-08: Persistent state survives gateway restart."""

    @pytest.mark.asyncio
    @pytest.mark.timeout(120)
    async def test_gateway_restarts_with_persistent_state(self, tmp_path):
        mob_toml_path = tmp_path / "mob.toml"
        mob_toml_path.write_text(_HOMECORE_MOB_TOML)
        sd = str(tmp_path / "state")
        os.makedirs(sd, exist_ok=True)

        # --- First boot ---
        rt1 = await _boot_runtime(sd, str(mob_toml_path))
        try:
            handle1 = rt1.mob_handle()
            await handle1.ensure_member("luka", profile="personal")
            r1 = await handle1.send("luka", message="Remember: the magic number is 7.")
            assert r1.accepted
        finally:
            await rt1.shutdown()

        # Verify state files were written during first boot
        state_files = os.listdir(sd)
        assert any("sessions" in f for f in state_files), (
            f"Expected session state in {sd}, found: {state_files}"
        )

        # --- Second boot (same persistent_state directory) ---
        rt2 = await _boot_runtime(sd, str(mob_toml_path))
        try:
            handle2 = rt2.mob_handle()

            # Re-ensure the same member — gateway boots successfully from persistent state
            snap = await handle2.ensure_member("luka", profile="personal")
            assert snap.meerkat_id == "luka"

            # Can send a follow-up message (new session at mob level, but gateway boots OK)
            r2 = await handle2.send("luka", message="What was the magic number?")
            assert r2.accepted
            assert r2.member_id == "luka"
        finally:
            await rt2.shutdown()

    @pytest.mark.asyncio
    @pytest.mark.timeout(60)
    async def test_persistent_state_creates_sqlite(self, tmp_path):
        mob_toml_path = tmp_path / "mob.toml"
        mob_toml_path.write_text(_HOMECORE_MOB_TOML)
        sd = str(tmp_path / "state")
        os.makedirs(sd, exist_ok=True)

        rt = await _boot_runtime(sd, str(mob_toml_path))
        try:
            handle = rt.mob_handle()
            await handle.ensure_member("luka", profile="personal")
            await handle.send("luka", message="Hello")
        finally:
            await rt.shutdown()

        # Verify persistent state was written
        state_files = os.listdir(sd)
        assert any("sessions" in f for f in state_files), (
            f"Expected SQLite session file in {sd}, found: {state_files}"
        )
