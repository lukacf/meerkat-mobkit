"""Tests for the MobKit error hierarchy."""
import pytest

from meerkat_mobkit.errors import (
    CapabilityUnavailableError,
    ContractMismatchError,
    MobKitError,
    NotConnectedError,
    RpcError,
    TransportError,
)


class TestMobKitErrorHierarchy:
    def test_base_is_exception(self):
        assert issubclass(MobKitError, Exception)

    @pytest.mark.parametrize(
        "cls",
        [
            TransportError,
            RpcError,
            CapabilityUnavailableError,
            ContractMismatchError,
            NotConnectedError,
        ],
    )
    def test_subclasses(self, cls):
        assert issubclass(cls, MobKitError)


class TestRpcError:
    def test_attributes(self):
        err = RpcError(42, "bad request", request_id="req-1", method="status")
        assert err.code == 42
        assert err.request_id == "req-1"
        assert err.method == "status"

    def test_message_via_str(self):
        err = RpcError(1, "something broke")
        assert str(err) == "something broke"
