#!/usr/bin/env python3
"""Contract test: the decorator-conformance gate fails on the defect it was
written for (a production decorator missing a defaulted method), ignores test
doubles, respects feature gates, checks every trait in `TRAIT_SPECS`, and only
honours an exemption that names a method the trait still has."""

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


RUNTIME_STORE_TRAIT_SRC = '''
pub trait RuntimeStore: RuntimeSessionAuthorityOps + Send + Sync {
    fn session_authority_ops(&self) -> &dyn RuntimeSessionAuthorityOps;
    async fn commit_prepared_session_boundary_with_fence(
        &self,
        _runtime_id: &LogicalRuntimeId,
    ) -> Result<(), RuntimeStoreError> {
        Err(RuntimeStoreError::Unsupported(
            "commit_prepared_session_boundary_with_fence".to_string(),
        ))
    }
    async fn list_runtime_delivery_authorities(
        &self,
    ) -> Result<Vec<(LogicalRuntimeId, RuntimeDeliveryAuthorityRecord)>, RuntimeStoreError> {
        Err(RuntimeStoreError::Unsupported(
            "list_runtime_delivery_authorities".into(),
        ))
    }
}

pub trait RuntimeStoreWriteFence: Send + Sync {
    fn unrelated(&self);
}
'''

RUNTIME_STORE_IMPL_SRC = '''
#[async_trait]
impl meerkat_runtime::RuntimeStore for SessionStoreBackedRuntimeStore {
    fn session_authority_ops(&self) -> &dyn meerkat_runtime::store::RuntimeSessionAuthorityOps {
        self.inner.session_authority_ops()
    }
}
'''


class DecoratorConformanceGate(unittest.TestCase):
    def test_runtime_store_spec_is_checked_and_exemption_needs_a_reason(self) -> None:
        spec = next(s for s in gate.TRAIT_SPECS if s.name == "RuntimeStore")
        methods = gate.trait_methods(RUNTIME_STORE_TRAIT_SRC, spec.name)
        # `\b` keeps `RuntimeStoreWriteFence` out of the RuntimeStore trait body.
        self.assertEqual(
            set(methods),
            {"session_authority_ops", "commit_prepared_session_boundary_with_fence", "list_runtime_delivery_authorities"},
        )
        impls = gate.production_impls(RUNTIME_STORE_IMPL_SRC, spec.name)
        self.assertEqual([t for t, _ in impls], ["SessionStoreBackedRuntimeStore"])
        names = impls[0][1]
        # The 0.8.32 defect: the facade lacked the cross-runtime delivery read.
        # The fenced commit is exempt WITH a reason and must not be reported.
        self.assertEqual(
            gate.missing_methods(methods, set(), names, spec.exempt),
            ["list_runtime_delivery_authorities"],
        )
        self.assertIn("commit_prepared_session_boundary_with_fence", spec.exempt)
        self.assertTrue(spec.exempt["commit_prepared_session_boundary_with_fence"].strip())
        # Without the exemption the fenced commit is reported like any other.
        self.assertEqual(
            gate.missing_methods(methods, set(), names),
            ["commit_prepared_session_boundary_with_fence", "list_runtime_delivery_authorities"],
        )
        # An exemption naming a method the trait no longer has is itself a failure.
        self.assertEqual(gate.stale_exemptions(methods, {"gone_method": "why"}), ["gone_method"])
        self.assertEqual(gate.stale_exemptions(methods, spec.exempt), [])

    def test_every_spec_is_addressed_by_name(self) -> None:
        self.assertEqual([s.name for s in gate.TRAIT_SPECS], ["MobSessionService", "RuntimeStore"])
        self.assertEqual(gate.TRAIT_SPECS[0].exempt, {})

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
