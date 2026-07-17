#!/usr/bin/env python3
"""Verify that one MobKit release is publicly readable from every registry."""

from __future__ import annotations

import argparse
import json
import time
from typing import Any, Callable
from urllib.error import HTTPError, URLError
from urllib.parse import quote
from urllib.request import Request, urlopen


CRATE_NAME = "meerkat-mobkit"
PYPI_NAME = "meerkat-mobkit"
NPM_NAME = "@rkat/mobkit-sdk"


class RegistryVerificationError(RuntimeError):
    """One or more public registries do not expose the requested release."""


def _read_json(url: str, *, timeout: float = 30.0) -> dict[str, Any]:
    request = Request(
        url,
        headers={
            "Accept": "application/json",
            "User-Agent": "meerkat-mobkit-release-verifier",
        },
    )
    with urlopen(request, timeout=timeout) as response:
        payload = json.load(response)
    if not isinstance(payload, dict):
        raise RegistryVerificationError(f"{url} returned a non-object JSON payload")
    return payload


def _verify_crates_payload(payload: dict[str, Any], version: str) -> None:
    published = payload.get("version")
    if not isinstance(published, dict):
        raise RegistryVerificationError("crates.io response is missing version metadata")
    if published.get("crate") != CRATE_NAME or published.get("num") != version:
        raise RegistryVerificationError(
            "crates.io returned unexpected package/version metadata"
        )


def _verify_pypi_payload(payload: dict[str, Any], version: str) -> None:
    info = payload.get("info")
    if not isinstance(info, dict) or info.get("version") != version:
        raise RegistryVerificationError("PyPI returned an unexpected package version")

    files = payload.get("urls")
    if not isinstance(files, list):
        raise RegistryVerificationError("PyPI response is missing release artifacts")
    package_types = {
        item.get("packagetype")
        for item in files
        if isinstance(item, dict) and item.get("filename")
    }
    missing = {"bdist_wheel", "sdist"} - package_types
    if missing:
        raise RegistryVerificationError(
            "PyPI release is missing expected artifact types: "
            + ", ".join(sorted(missing))
        )


def _verify_npm_payload(payload: dict[str, Any], version: str) -> None:
    if payload.get("name") != NPM_NAME or payload.get("version") != version:
        raise RegistryVerificationError("npm returned unexpected package/version metadata")


def _registry_checks(
    version: str,
) -> tuple[tuple[str, str, Callable[[dict[str, Any], str], None]], ...]:
    encoded_crate = quote(CRATE_NAME, safe="")
    encoded_pypi = quote(PYPI_NAME, safe="")
    encoded_npm = quote(NPM_NAME, safe="")
    encoded_version = quote(version, safe="")
    return (
        (
            "crates.io",
            f"https://crates.io/api/v1/crates/{encoded_crate}/{encoded_version}",
            _verify_crates_payload,
        ),
        (
            "PyPI",
            f"https://pypi.org/pypi/{encoded_pypi}/{encoded_version}/json",
            _verify_pypi_payload,
        ),
        (
            "npm",
            f"https://registry.npmjs.org/{encoded_npm}/{encoded_version}",
            _verify_npm_payload,
        ),
    )


def verify_once(
    version: str,
    *,
    reader: Callable[[str], dict[str, Any]] = _read_json,
) -> None:
    failures: list[str] = []
    for registry, url, validator in _registry_checks(version):
        try:
            validator(reader(url), version)
        except (
            RegistryVerificationError,
            HTTPError,
            URLError,
            TimeoutError,
            OSError,
            ValueError,
        ) as error:
            failures.append(f"{registry}: {error}")
    if failures:
        raise RegistryVerificationError("; ".join(failures))


def verify_with_retries(
    version: str,
    *,
    attempts: int = 30,
    delay_seconds: float = 10.0,
    reader: Callable[[str], dict[str, Any]] = _read_json,
    sleeper: Callable[[float], None] = time.sleep,
) -> None:
    if attempts < 1:
        raise ValueError("attempts must be at least 1")
    if delay_seconds < 0:
        raise ValueError("delay_seconds must not be negative")

    last_error: RegistryVerificationError | None = None
    for attempt in range(1, attempts + 1):
        try:
            verify_once(version, reader=reader)
        except RegistryVerificationError as error:
            last_error = error
            if attempt == attempts:
                break
            print(
                f"Registry verification attempt {attempt}/{attempts} incomplete: {error}"
            )
            sleeper(delay_seconds)
        else:
            print(
                f"Verified {CRATE_NAME} {version} on crates.io, "
                f"{PYPI_NAME} {version} wheel+sdist on PyPI, and "
                f"{NPM_NAME} {version} on npm"
            )
            return

    assert last_error is not None
    raise RegistryVerificationError(
        f"release {version} was not fully public after {attempts} attempts: {last_error}"
    ) from last_error


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--attempts", type=int, default=30)
    parser.add_argument("--delay-seconds", type=float, default=10.0)
    args = parser.parse_args()

    version = args.version.strip().removeprefix("v")
    if not version:
        parser.error("--version must not be empty")
    try:
        verify_with_retries(
            version,
            attempts=args.attempts,
            delay_seconds=args.delay_seconds,
        )
    except (RegistryVerificationError, ValueError) as error:
        parser.exit(1, f"registry verification failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
