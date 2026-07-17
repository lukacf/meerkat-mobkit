#!/usr/bin/env python3
"""Tests for exact public-registry release verification."""

from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest


SCRIPT = Path(__file__).with_name("verify-published-registries.py")
SPEC = importlib.util.spec_from_file_location("verify_published_registries", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
registries = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(registries)


def exact_payload(url: str) -> dict:
    if "crates.io" in url:
        return {"version": {"crate": "meerkat-mobkit", "num": "0.8.0"}}
    if "pypi.org" in url:
        return {
            "info": {"version": "0.8.0"},
            "urls": [
                {"filename": "meerkat_mobkit-0.8.0-py3-none-any.whl", "packagetype": "bdist_wheel"},
                {"filename": "meerkat_mobkit-0.8.0.tar.gz", "packagetype": "sdist"},
            ],
        }
    if "npmjs.org" in url:
        return {"name": "@rkat/mobkit-sdk", "version": "0.8.0"}
    raise AssertionError(f"unexpected URL: {url}")


class PublishedRegistryTests(unittest.TestCase):
    def test_exact_release_is_verified_across_all_registries(self):
        registries.verify_once("0.8.0", reader=exact_payload)

    def test_pypi_requires_both_wheel_and_source_distribution(self):
        def missing_sdist(url: str) -> dict:
            payload = exact_payload(url)
            if "pypi.org" in url:
                payload["urls"] = payload["urls"][:1]
            return payload

        with self.assertRaisesRegex(
            registries.RegistryVerificationError, "sdist"
        ):
            registries.verify_once("0.8.0", reader=missing_sdist)

    def test_retry_allows_registry_propagation_but_still_requires_exact_artifacts(self):
        calls = 0
        sleeps: list[float] = []

        def propagating(url: str) -> dict:
            nonlocal calls
            attempt = calls // 3
            calls += 1
            payload = exact_payload(url)
            if attempt == 0 and "pypi.org" in url:
                payload["urls"] = payload["urls"][:1]
            return payload

        registries.verify_with_retries(
            "0.8.0",
            attempts=2,
            delay_seconds=0.25,
            reader=propagating,
            sleeper=sleeps.append,
        )

        self.assertEqual(calls, 6)
        self.assertEqual(sleeps, [0.25])

    def test_retry_exhaustion_fails_the_release(self):
        def unavailable(url: str) -> dict:
            raise OSError(f"not public: {url}")

        with self.assertRaisesRegex(
            registries.RegistryVerificationError, "not fully public after 2 attempts"
        ):
            registries.verify_with_retries(
                "0.8.0",
                attempts=2,
                delay_seconds=0,
                reader=unavailable,
                sleeper=lambda _: None,
            )


if __name__ == "__main__":
    unittest.main()
