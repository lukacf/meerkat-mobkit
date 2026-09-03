#!/usr/bin/env python3
"""Contract test: the decorator-conformance gate fails on the defect it was
written for (a production decorator missing a defaulted method), ignores test
doubles, and respects feature gates."""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path

SPEC = importlib.util.spec_from_file_location(
    "gate", Path(__file__).with_name("verify-session-service-decorators.py")
)
gate = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(gate)

TRAIT_SRC = '''
pub trait MobSessionService:
    Send + Sync + SomeOtherTrait
{
    async fn required_one(&self) -> Result<(), SessionError>;
    async fn defaulted_refusal(&self) -> Result<(), SessionError> {
        Err(SessionError::Unsupported("nope".into()))
    }
    #[cfg(feature = "experimental-gpt-live")]
    async fn live_only(&self) -> Result<(), SessionError> {
        Err(SessionError::Unsupported("nope".into()))
    }
    fn plain_default(&self) -> bool {
        false
    }
}
'''

IMPL_SRC = '''
impl MobSessionService for GoodWrapper {
    async fn required_one(&self) -> Result<(), SessionError> { self.inner.required_one().await }
    async fn defaulted_refusal(&self) -> Result<(), SessionError> { self.inner.defaulted_refusal().await }
    fn plain_default(&self) -> bool { self.inner.plain_default() }
}

impl MobSessionService for ForgetfulWrapper {
    async fn required_one(&self) -> Result<(), SessionError> { self.inner.required_one().await }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    impl MobSessionService for TestDouble {
        async fn required_one(&self) -> Result<(), SessionError> { Ok(()) }
    }
}
'''


class DecoratorConformanceGate(unittest.TestCase):
    def test_trait_methods_and_feature_gates(self) -> None:
        methods = gate.trait_methods(TRAIT_SRC)
        self.assertEqual(
            methods,
            {"required_one": None, "defaulted_refusal": None, "live_only": "experimental-gpt-live", "plain_default": None},
        )

    def test_forgetful_production_decorator_fails_and_test_double_is_ignored(self) -> None:
        methods = gate.trait_methods(TRAIT_SRC)
        impls = gate.production_impls(IMPL_SRC)
        self.assertEqual([t for t, _ in impls], ["GoodWrapper", "ForgetfulWrapper"], "the cfg(test) double must not count")
        by_target = dict(impls)
        self.assertEqual(gate.missing_methods(methods, set(), by_target["GoodWrapper"]), [])
        self.assertEqual(
            gate.missing_methods(methods, set(), by_target["ForgetfulWrapper"]),
            ["defaulted_refusal", "plain_default"],
        )

    def test_feature_gated_method_counts_only_when_enabled(self) -> None:
        methods = gate.trait_methods(TRAIT_SRC)
        names = {"required_one", "defaulted_refusal", "plain_default"}
        self.assertEqual(gate.missing_methods(methods, set(), names), [])
        self.assertEqual(gate.missing_methods(methods, {"experimental-gpt-live"}, names), ["live_only"])


if __name__ == "__main__":
    unittest.main()
