"""Tests for the child application tool policy declaration.

Once compiled application tool policies are served, Meerkat refuses a
member's ``mob_create`` and ``delegate`` until the host chooses the policy
child mob members run under. HomeCore composes through this SDK only, so the
key must be reachable from the builder and absent unless the host chose one.
"""

import pytest

from meerkat_mobkit.builder import MobKit
from meerkat_mobkit.runtime import MobKitRuntime


def _init_params(builder):
    return MobKitRuntime(builder._config)._build_init_params()


HOUSEHOLD = {
    "kind": "provider",
    "provider_id": "homecore",
    "policy_id": "household-tools",
}


def test_a_provider_binding_reaches_the_init_params():
    builder = MobKit.builder().child_application_tool_policy(HOUSEHOLD)
    assert _init_params(builder)["child_application_tool_policy"] == HOUSEHOLD


def test_the_explicit_unmanaged_opt_out_reaches_the_init_params():
    builder = MobKit.builder().child_application_tool_policy({"kind": "unmanaged"})
    assert _init_params(builder)["child_application_tool_policy"] == {
        "kind": "unmanaged"
    }


def test_the_parameter_is_absent_unless_the_host_chose_one():
    assert "child_application_tool_policy" not in _init_params(MobKit.builder())


def test_the_builder_keeps_its_own_copy():
    binding = dict(HOUSEHOLD)
    builder = MobKit.builder().child_application_tool_policy(binding)
    binding["policy_id"] = "guest-tools"
    assert (
        _init_params(builder)["child_application_tool_policy"]["policy_id"]
        == "household-tools"
    )


@pytest.mark.parametrize(
    ("binding", "message"),
    [
        ({"kind": "inherit"}, "inherit is not valid for child mobs"),
        ({}, "kind must be 'provider' or 'unmanaged'"),
        ({"kind": "unmanaged", "provider_id": "homecore"}, "takes no other keys"),
        ({"kind": "provider", "provider_id": "homecore"}, "needs exactly"),
        (
            {"kind": "provider", "provider_id": "homecore", "policy_id": " "},
            "policy_id must be a non-empty string",
        ),
        (
            {"kind": "provider", "provider_id": 7, "policy_id": "household-tools"},
            "provider_id must be a non-empty string",
        ),
    ],
)
def test_a_binding_meerkat_cannot_use_is_refused(binding, message):
    with pytest.raises(ValueError, match=message):
        MobKit.builder().child_application_tool_policy(binding)


def test_a_non_mapping_is_refused():
    with pytest.raises(TypeError, match="must be a mapping"):
        MobKit.builder().child_application_tool_policy("unmanaged")
