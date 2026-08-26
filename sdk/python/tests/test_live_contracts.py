import json
from pathlib import Path

import pytest

from meerkat_mobkit.live import (
    LIVE_EXECUTION_CLIENT_CONTEXT_V1,
    LIVE_EXECUTION_FUNCTION_BRIDGE_V1,
    LIVE_EXECUTION_IDENTITY_V1,
    ActiveLiveChannelHandle,
    ExperimentalLiveChannelStatus,
    ExperimentalLiveGatewayConfig,
    LiveAssistantOutputAddress,
    LiveAuthBindingOverride,
    LiveAuthBindingRef,
    LiveChannelHandle,
    LiveExecutionIdentityV1,
    LivePlaybackCompleteResult,
    LivePlaybackOwnerReadiness,
    PendingLiveChannelHandle,
    LiveReplacementRequired,
    live_open_execution_identity_params,
    live_execution_mode_capability,
    supports_live_execution_mode,
    supports_live_execution_identity_v1,
)
from meerkat_mobkit.types import CapabilitiesResult


@pytest.fixture(scope="module")
def contracts_fixture():
    path = Path(__file__).parents[3] / "meerkat-mobkit/tests/fixtures/live_contracts_v1.json"
    return json.loads(path.read_text(encoding="utf-8"))


def test_experimental_live_gateway_registration_is_explicit_and_strict():
    config = ExperimentalLiveGatewayConfig(
        principal="user:luka",
        realm="family",
        factory_kind="openai-gpt-live",
        factory_version="v1",
        gate0_qualification="gate0-v1",
        auth_binding=LiveAuthBindingRef(
            realm="family", binding="chatgpt-oauth", profile="luka"
        ),
        voice="marin",
    )
    assert config.to_dict() == {
        "principal": "user:luka",
        "realm": "family",
        "factory_kind": "openai-gpt-live",
        "factory_version": "v1",
        "gate0_qualification": "gate0-v1",
        "auth_binding": {
            "realm": "family",
            "binding": "chatgpt-oauth",
            "profile": "luka",
        },
        "voice": "marin",
    }
    with pytest.raises(ValueError, match="realm must equal"):
        ExperimentalLiveGatewayConfig(
            principal="user:luka",
            realm="family",
            factory_kind="openai-gpt-live",
            factory_version="v1",
            gate0_qualification="gate0-v1",
            auth_binding=LiveAuthBindingRef(
                realm="other", binding="chatgpt-oauth"
            ),
            voice="marin",
        ).to_dict()


def test_execution_identity_matches_shared_set_and_clear_fixtures(contracts_fixture):
    selected = LiveExecutionIdentityV1(
        model="gpt-live-1-codex",
        provider="openai",
        auth_binding=LiveAuthBindingOverride.set(
            LiveAuthBindingRef(
                realm="family", binding="chatgpt-oauth", profile="luka"
            )
        ),
    )
    cleared = LiveExecutionIdentityV1(
        model="gpt-live-1-codex",
        provider="openai",
        auth_binding=LiveAuthBindingOverride.clear(),
    )
    assert selected.to_dict() == contracts_fixture["execution_identity_set"]
    assert cleared.to_dict() == contracts_fixture["execution_identity_clear"]
    assert LiveExecutionIdentityV1.from_dict(selected.to_dict()) == selected


def test_execution_identity_rejects_unknown_fields_null_and_legacy_conflicts():
    with pytest.raises(ValueError, match="unknown field"):
        LiveExecutionIdentityV1.from_dict(
            {"version": "v1", "model": "gpt-live-1-codex", "extra": True}
        )
    with pytest.raises(ValueError, match="cannot be null"):
        LiveExecutionIdentityV1.from_dict({"version": "v1", "auth_binding": None})
    with pytest.raises(ValueError, match="legacy top-level model"):
        live_open_execution_identity_params(
            LiveExecutionIdentityV1(model="gpt-live-1-codex"), model="legacy"
        )
    with pytest.raises(ValueError, match="must be v1"):
        LiveExecutionIdentityV1.from_dict(
            {"version": "v2", "model": "gpt-live-1-codex"}
        )
    with pytest.raises(ValueError, match="must be v1"):
        LiveExecutionIdentityV1.from_dict({"model": "gpt-live-1-codex"})
    for field, value in [
        ("model", None),
        ("model", "  "),
        ("provider", None),
        ("self_hosted_server_id", None),
        ("self_hosted_server_id", "  "),
    ]:
        with pytest.raises(ValueError):
            LiveExecutionIdentityV1.from_dict({"version": "v1", field: value})
    with pytest.raises(ValueError):
        LiveExecutionIdentityV1(
            model="gpt-live-1-codex",
            auth_binding=LiveAuthBindingOverride.set(
                LiveAuthBindingRef(realm="family", binding="chatgpt", profile="  ")
            ),
        ).to_dict()


def test_capability_is_off_when_absent():
    old_gateway = CapabilitiesResult.from_dict(
        {"contract_version": "0.5.0", "methods": [], "loaded_modules": []}
    )
    assert old_gateway.feature_capabilities == []
    assert not supports_live_execution_identity_v1(old_gateway.feature_capabilities)
    assert supports_live_execution_identity_v1([LIVE_EXECUTION_IDENTITY_V1])


def test_channel_handle_matches_shared_fixture(contracts_fixture):
    wire = contracts_fixture["channel_handle"]
    handle = LiveChannelHandle.from_dict(wire)
    assert handle.channel_id == "live-ch-1"
    assert handle.target_identity == "identity:luka"
    assert handle.transport.transport == "websocket"
    assert handle.continuity.mode == "transcript_only"
    assert handle.to_dict() == wire


def test_pending_active_and_readiness_handles_are_distinct_and_strict(
    contracts_fixture,
):
    pending = PendingLiveChannelHandle.from_dict(
        contracts_fixture["pending_channel_handle"]
    )
    active = ActiveLiveChannelHandle.from_dict(
        contracts_fixture["active_channel_handle"]
    )
    readiness = LivePlaybackOwnerReadiness.from_dict(
        contracts_fixture["playback_owner_readiness"]
    )
    assert pending.execution_mode == "function_bridge"
    assert active.channel_id == pending.channel_id == readiness.channel_id
    assert active.activation_receipt != pending.pending_receipt
    assert pending.to_dict() == contracts_fixture["pending_channel_handle"]
    assert active.to_dict() == contracts_fixture["active_channel_handle"]
    assert readiness.to_dict() == contracts_fixture["playback_owner_readiness"]
    assert ExperimentalLiveChannelStatus.from_dict(
        {"phase": "active", "handle": active.to_dict()}
    ).handle == active
    for key in [
        "pending_status",
        "active_status",
        "revoked_status",
        "closed_status",
    ]:
        assert (
            ExperimentalLiveChannelStatus.from_dict(contracts_fixture[key]).to_dict()
            == contracts_fixture[key]
        )

    for drifted in [
        {**active.to_dict(), "execution_mode": "responses"},
        {**active.to_dict(), "responses_model": "gpt-5.5"},
        {**pending.to_dict(), "tools": []},
        {**active.to_dict(), "activation_receipt": "  "},
    ]:
        parser = (
            PendingLiveChannelHandle.from_dict
            if "pending_receipt" in drifted
            else ActiveLiveChannelHandle.from_dict
        )
        with pytest.raises(ValueError):
            parser(drifted)


def test_provider_neutral_mode_capabilities_are_independent():
    advertised = [LIVE_EXECUTION_IDENTITY_V1, LIVE_EXECUTION_FUNCTION_BRIDGE_V1]
    assert supports_live_execution_mode(advertised, "function_bridge")
    assert not supports_live_execution_mode(advertised, "client_context")
    assert (
        live_execution_mode_capability("client_context")
        == LIVE_EXECUTION_CLIENT_CONTEXT_V1
    )
    with pytest.raises(ValueError):
        live_execution_mode_capability("responses")  # type: ignore[arg-type]


def test_replacement_required_is_strict_and_preserves_fresh_handle(contracts_fixture):
    wire = {
        "required": True,
        "reason": "canonical_context",
        "replacement": contracts_fixture["channel_handle"],
        "canonical_seed_cursor": 17,
    }
    result = LiveReplacementRequired.from_dict(wire)
    assert result.required is True
    assert result.replacement is not None
    assert result.replacement.channel_id == "live-ch-1"
    assert result.to_dict() == wire
    assert LiveReplacementRequired.from_dict({"required": False}).to_dict() == {
        "required": False
    }
    with pytest.raises(ValueError, match="unknown field"):
        LiveReplacementRequired.from_dict(
            {"required": False, "channel_id": "stale-old-channel"}
        )


def test_playback_complete_result_is_strict():
    assert LivePlaybackCompleteResult.from_dict(
        {"status": "completed"}
    ).to_dict() == {"status": "completed"}
    with pytest.raises(ValueError):
        LivePlaybackCompleteResult.from_dict(
            {"status": "completed", "interaction_id": "caller-minted"}
        )


def test_assistant_output_address_is_strict_and_opaque():
    wire = {
        "channel_id": "live-ch-1",
        "output_id": "opaque-output-1",
        "content_index": 0,
    }
    assert LiveAssistantOutputAddress.from_dict(wire).to_dict() == wire
    with pytest.raises(ValueError, match="unknown field"):
        LiveAssistantOutputAddress.from_dict({**wire, "item_id": "provider-item"})
    with pytest.raises(ValueError, match="non-negative integer"):
        LiveAssistantOutputAddress.from_dict({**wire, "content_index": -1})
