"""Tests for meerkat_mobkit.config.memory backend configs."""
from __future__ import annotations

import warnings

import pytest

from meerkat_mobkit.config import memory


class TestLocalJson:
    def test_to_dict_without_health_endpoint(self):
        config = memory.local_json()
        assert config.to_dict() == {"backend": "local_json"}

    def test_to_dict_with_health_endpoint(self):
        config = memory.local_json(health_check_endpoint="http://localhost:3000")
        assert config.to_dict() == {
            "backend": "local_json",
            "health_check_endpoint": "http://localhost:3000",
        }


class TestElephantDeprecatedAlias:
    def test_emits_deprecation_warning_and_keeps_legacy_wire_shape(self):
        with pytest.warns(DeprecationWarning, match="local_json"):
            config = memory.elephant("http://localhost:3000")
        assert config.to_dict() == {
            "backend": "elephant",
            "endpoint": "http://localhost:3000",
        }

    def test_warns_about_ignored_fields(self):
        with warnings.catch_warnings(record=True) as caught:
            warnings.simplefilter("always")
            config = memory.elephant(
                "http://localhost:3000",
                space_id="sp-1",
                collection="col-1",
                stores=["knowledge_graph"],
            )
        messages = [str(w.message) for w in caught]
        assert any("collection, space_id, stores" in message for message in messages)
        # Ignored fields must stay out of the wire payload either way.
        assert config.to_dict() == {
            "backend": "elephant",
            "endpoint": "http://localhost:3000",
        }
