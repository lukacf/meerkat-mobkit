#!/usr/bin/env python3
"""Focused behavioral tests for registry publication in the release workflow."""

import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
RELEASE_WORKFLOW = ROOT / ".github/workflows/release.yml"


def typescript_publish_script() -> str:
    """Extract the TypeScript publication shell from the workflow verbatim."""
    lines = RELEASE_WORKFLOW.read_text().splitlines()
    step = lines.index("      - name: Publish TypeScript SDK")
    run = lines.index("        run: |", step)
    script_lines: list[str] = []
    for line in lines[run + 1 :]:
        if line and not line.startswith("          "):
            break
        script_lines.append(line[10:] if line else "")

    # GitHub resolves this expression before invoking the shell. Tests expose the
    # resolved value through an environment variable instead.
    script_lines = [
        'DRY_RUN="${REGISTRY_DRY_RUN:-false}"'
        if line.startswith('DRY_RUN="${{')
        else line
        for line in script_lines
    ]
    return "\n".join(script_lines)


class NpmReleaseWorkflowTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.bin = self.root / "bin"
        self.sdk = self.root / "sdk/typescript"
        self.bin.mkdir()
        self.sdk.mkdir(parents=True)
        (self.sdk / "package.json").write_text(
            json.dumps({"name": "@rkat/mobkit-sdk", "version": "0.8.0"}) + "\n"
        )
        self.log = self.root / "npm.log"
        npm = self.bin / "npm"
        npm.write_text(
            """#!/usr/bin/env bash
set -euo pipefail
command="$1"
shift
printf '%s\\n' "$command $*" >> "$FAKE_NPM_LOG"
case "$command" in
  install|config|run)
    exit 0
    ;;
  view)
    case "$FAKE_NPM_VIEW" in
      exact)
        printf '%s\\n' "$FAKE_NPM_VERSION"
        ;;
      mismatch)
        printf '%s\\n' "9.9.9"
        ;;
      missing)
        echo "npm error code E404" >&2
        echo "npm error 404 No match found for version 0.8.0" >&2
        exit 1
        ;;
      auth)
        echo "npm error code E401" >&2
        echo "npm error Unable to authenticate" >&2
        exit 1
        ;;
      network)
        echo "npm error code EAI_AGAIN" >&2
        exit 1
        ;;
    esac
    ;;
  publish)
    if [[ "${FAKE_NPM_PUBLISH_STATUS:-0}" != "0" ]]; then
      echo "npm error publish failed" >&2
      exit "$FAKE_NPM_PUBLISH_STATUS"
    fi
    ;;
esac
"""
        )
        npm.chmod(0o755)

    def tearDown(self):
        self.temp.cleanup()

    def run_publish(
        self,
        view: str,
        *,
        dry_run: bool = False,
        publish_status: int = 0,
    ) -> subprocess.CompletedProcess[str]:
        env = os.environ.copy()
        env.update(
            {
                "PATH": f"{self.bin}:{env['PATH']}",
                "NODE_AUTH_TOKEN": "test-token",
                "REGISTRY_DRY_RUN": "true" if dry_run else "false",
                "FAKE_NPM_LOG": str(self.log),
                "FAKE_NPM_VIEW": view,
                "FAKE_NPM_VERSION": "0.8.0",
                "FAKE_NPM_PUBLISH_STATUS": str(publish_status),
            }
        )
        return subprocess.run(
            ["bash", "-c", typescript_publish_script()],
            cwd=self.sdk,
            env=env,
            capture_output=True,
            text=True,
        )

    def npm_commands(self) -> list[str]:
        return self.log.read_text().splitlines()

    def test_exact_published_version_is_skipped(self):
        result = self.run_publish("exact")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("@rkat/mobkit-sdk@0.8.0: already published, skipping", result.stdout)
        self.assertTrue(any(command.startswith("view ") for command in self.npm_commands()))
        self.assertFalse(any(command.startswith("publish ") for command in self.npm_commands()))

    def test_explicit_exact_version_404_publishes(self):
        result = self.run_publish("missing")

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(any(command.startswith("publish ") for command in self.npm_commands()))

    def test_registry_auth_failure_is_not_treated_as_missing(self):
        result = self.run_publish("auth")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("E401", result.stderr)
        self.assertFalse(any(command.startswith("publish ") for command in self.npm_commands()))

    def test_unexpected_exact_query_response_fails_closed(self):
        result = self.run_publish("mismatch")

        self.assertNotEqual(result.returncode, 0)
        self.assertIn("unexpected version", result.stderr)
        self.assertFalse(any(command.startswith("publish ") for command in self.npm_commands()))

    def test_genuine_publish_failure_is_preserved(self):
        result = self.run_publish("missing", publish_status=23)

        self.assertEqual(result.returncode, 23)
        self.assertIn("publish failed", result.stderr)


class RegistryReadbackWorkflowTests(unittest.TestCase):
    def test_exact_registry_readback_runs_after_all_publish_steps(self):
        workflow = RELEASE_WORKFLOW.read_text()
        verification = "      - name: Verify exact packages are public on every registry"

        self.assertIn(verification, workflow)
        self.assertLess(workflow.index("      - name: Publish Rust crate"), workflow.index(verification))
        self.assertLess(workflow.index("      - name: Publish Python SDK"), workflow.index(verification))
        self.assertLess(workflow.index("      - name: Publish TypeScript SDK"), workflow.index(verification))
        self.assertIn("python scripts/verify-published-registries.py --version", workflow)
        self.assertIn("github.event.inputs.registry_dry_run != 'true'", workflow)


if __name__ == "__main__":
    unittest.main()
