"""Tests for boot-scoped member role migration declarations.

A durable member whose role changed refuses to resume until the host declares
the migration. These tests pin the only two things the SDK owes that contract:
the declaration is REACHABLE from Python (HomeCore found it was not, which made
the whole carrier unusable from the SDK they run), and it is ABSENT unless the
host asked for it.
"""
import json
from pathlib import Path

import pytest

from meerkat_mobkit import RoleMigrationDeclaration
from meerkat_mobkit.builder import MobKit
from meerkat_mobkit.runtime import MobKitRuntime


def _init_params(builder):
    return MobKitRuntime(builder._config)._build_init_params()


def test_declaration_reaches_the_init_params():
    builder = MobKit.builder().role_migrations(
        [
            RoleMigrationDeclaration(
                identity="domain:home-automation", from_role="domain"
            )
        ]
    )
    params = _init_params(builder)
    assert params["role_migrations"] == [
        {"identity": "domain:home-automation", "from_role": "domain"}
    ]


def test_a_plain_dict_is_accepted_and_still_validated():
    builder = MobKit.builder().role_migrations(
        [{"identity": "domain:home-automation", "from_role": "domain"}]
    )
    assert _init_params(builder)["role_migrations"] == [
        {"identity": "domain:home-automation", "from_role": "domain"}
    ]

    with pytest.raises(ValueError):
        MobKit.builder().role_migrations([{"identity": "has a space", "from_role": "d"}])


def test_no_declaration_means_no_key_at_all():
    """Absent by default: a boot that declares nothing must arm nothing.

    The key must be absent rather than an empty list, so a gateway can never
    read an armed-but-empty declaration as "migrations were considered".
    """
    assert "role_migrations" not in _init_params(MobKit.builder())
    assert "role_migrations" not in _init_params(MobKit.builder().role_migrations([]))


# The one committed wire contract, shared with the Rust parser's
# `the_committed_wire_fixture_deserializes_into_declarations`. Renaming a key on
# either side goes red here or there, instead of both sides staying green while
# a host arms nothing.
FIXTURE = (
    Path(__file__).resolve().parents[3]
    / "meerkat-mobkit"
    / "tests"
    / "fixtures"
    / "role_migrations_init_params.json"
)


def test_builder_output_matches_the_committed_wire_fixture():
    expected = json.loads(FIXTURE.read_text())["role_migrations"]
    builder = MobKit.builder().role_migrations(
        [
            RoleMigrationDeclaration(
                identity=expected[0]["identity"], from_role=expected[0]["from_role"]
            )
        ]
    )
    assert _init_params(builder)["role_migrations"] == expected


def test_conflicting_declarations_are_refused_and_repeats_are_not():
    """A conflicting pair has no defensible resolution; a repeat is harmless.

    Refusing the identical repeat would be the failure mode of treating every
    irregularity as fatal.
    """
    with pytest.raises(ValueError, match="cannot be resolved by order"):
        MobKit.builder().role_migrations(
            [
                {"identity": "domain:home-automation", "from_role": "domain"},
                {"identity": "domain:home-automation", "from_role": "other"},
            ]
        )

    repeated = MobKit.builder().role_migrations(
        [
            {"identity": "domain:home-automation", "from_role": "domain"},
            {"identity": "domain:home-automation", "from_role": "domain"},
        ]
    )
    assert len(_init_params(repeated)["role_migrations"]) == 2
