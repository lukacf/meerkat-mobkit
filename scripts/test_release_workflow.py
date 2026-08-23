#!/usr/bin/env python3
"""Focused behavioral tests for registry publication in the release workflow."""

import json
import os
import re
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


def job_block(name: str) -> str:
    """The workflow text for exactly one job, bounded by the next job header.

    Slicing to end-of-file is only safe for the last job. `publish_sdk_packages`
    is followed by `publish_registries`, whose `if` already contains the very
    strings this file asserts about the SDK gate, so an unbounded slice would
    pass against unmodified code.
    """
    lines = RELEASE_WORKFLOW.read_text().splitlines()
    start = lines.index(f"  {name}:")
    for offset, line in enumerate(lines[start + 1 :], start=start + 1):
        if line.startswith("  ") and not line.startswith("   ") and line.rstrip().endswith(":"):
            return "\n".join(lines[start:offset])
    return "\n".join(lines[start:])


def job_condition(name: str) -> str:
    """The raw `if:` expression of one job, with block scalar folding undone."""
    block = job_block(name).splitlines()
    start = block.index("    if: >-")
    body: list[str] = []
    for line in block[start + 1 :]:
        if not line.startswith("      "):
            break
        body.append(line.strip())
    return " ".join(body)


def evaluate_condition(expression: str, **context) -> bool:
    """Evaluate a GitHub `if:` expression under an explicit context.

    String assertions cannot tell a conjunction from a disjunction, which is the
    entire defect class here, so the gate is exercised rather than pattern
    matched. Any name the expression uses that the caller did not supply raises,
    so a renamed input fails loudly instead of silently reading as falsey.
    """
    translated = (
        expression.replace("&&", " and ")
        .replace("||", " or ")
        .replace("always()", "True")
        .replace("!", " not ")
    )
    # Flatten dotted contexts BEFORE rewriting startsWith, or the rewrite's own
    # `.startswith` gets flattened into the identifier too.
    translated = re.sub(r"\b(github|needs)((?:\.[A-Za-z_][A-Za-z0-9_]*)+)",
                        lambda m: m.group(1) + m.group(2).replace(".", "_"), translated)
    translated = re.sub(
        r"startsWith\(([^,]+),\s*('[^']*')\)", r"\1.startswith(\2)", translated
    )

    missing = {
        name
        for name in re.findall(r"\b(?:github|needs)_[A-Za-z0-9_]+", translated)
        if name not in context
    }
    if missing:
        raise AssertionError(f"condition references unsupplied names: {sorted(missing)}")
    return bool(eval(translated, {"__builtins__": {}}, dict(context)))


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


class DocsPublicationWorkflowTests(unittest.TestCase):
    def test_docs_dispatch_requires_completed_public_release(self):
        workflow = RELEASE_WORKFLOW.read_text()
        docs_job = workflow.index("  publish_docs:")
        registry_readback = workflow.index(
            "      - name: Verify exact packages are public on every registry"
        )

        self.assertGreater(docs_job, registry_readback)
        self.assertIn(
            "needs: [publish_github_release, publish_registries]",
            workflow[docs_job:],
        )
        self.assertIn("needs.publish_github_release.result == 'success'", workflow[docs_job:])
        self.assertIn("needs.publish_registries.result == 'success'", workflow[docs_job:])
        self.assertIn("github.event.inputs.registry_dry_run != 'true'", workflow[docs_job:])

    def test_docs_dispatch_carries_immutable_release_identity(self):
        workflow = RELEASE_WORKFLOW.read_text()
        docs_job = workflow[workflow.index("  publish_docs:") :]

        self.assertIn("secrets.DOCS_PUBLISH_TOKEN", docs_job)
        self.assertIn('event_type: "mobkit-release-published"', docs_job)
        self.assertIn("tag: $tag", docs_job)
        self.assertIn("sha: $sha", docs_job)
        self.assertIn("version: $version", docs_job)
        self.assertIn("release_sha=$(git rev-parse HEAD)", docs_job)
        self.assertIn("repos/lukacf/meerkat/dispatches", docs_job)


class SdkPublicationOrderingTests(unittest.TestCase):
    """The SDK must not reach a public registry before the release it belongs to.

    A PyPI upload cannot be withdrawn and an npm unpublish is 72h-limited, while
    the five cross-compiles behind it can still fail afterwards. This repo has
    shipped that half-release already: the v0.6.43 tag dispatch published
    registries, then x86_64-pc-windows-msvc failed and the release assets were
    skipped. publish_registries was gated in response; splitting the SDK out of
    it re-opened the same hole for PyPI and npm alone.
    """

    TAG_RELEASE = dict(
        github_ref="refs/tags/v0.8.21",
        github_event_name="push",
        github_event_inputs_publish_release_packages="",
        github_event_inputs_registry_dry_run="",
        github_event_inputs_release_tag="",
        needs_release_validate_result="success",
    )

    def test_a_tagged_release_publishes_the_sdk_once_binaries_and_release_exist(self):
        self.assertTrue(
            evaluate_condition(
                job_condition("publish_sdk_packages"),
                **self.TAG_RELEASE,
                needs_build_binaries_result="success",
                needs_publish_github_release_result="success",
            )
        )

    def test_a_failed_cross_compile_withholds_the_sdk(self):
        # The v0.6.43 shape. Before the gate this published to PyPI and npm.
        self.assertFalse(
            evaluate_condition(
                job_condition("publish_sdk_packages"),
                **self.TAG_RELEASE,
                needs_build_binaries_result="failure",
                needs_publish_github_release_result="skipped",
            )
        )

    def test_a_missing_github_release_withholds_the_sdk(self):
        self.assertFalse(
            evaluate_condition(
                job_condition("publish_sdk_packages"),
                **self.TAG_RELEASE,
                needs_build_binaries_result="success",
                needs_publish_github_release_result="failure",
            )
        )

    def test_an_untagged_dispatch_cannot_reach_a_public_registry(self):
        # Dispatch from main with publish_release_packages=true and dry-run off.
        # build_binaries and publish_github_release are skipped by their own `if`
        # because there is no tag, so before the gate this uploaded a real
        # version to PyPI and npm off an untagged branch.
        self.assertFalse(
            evaluate_condition(
                job_condition("publish_sdk_packages"),
                github_ref="refs/heads/main",
                github_event_name="workflow_dispatch",
                github_event_inputs_publish_release_packages="true",
                github_event_inputs_registry_dry_run="false",
                github_event_inputs_release_tag="",
                needs_release_validate_result="success",
                needs_build_binaries_result="skipped",
                needs_publish_github_release_result="skipped",
            )
        )

    def test_the_dry_run_lane_still_exercises_the_sdk_build(self):
        # Deliberate: on a dry run the uploads are skipped inside the steps, so
        # the job must still run or `python -m build`, `twine check` and
        # `npm publish --dry-run` stop being exercised at all.
        self.assertTrue(
            evaluate_condition(
                job_condition("publish_sdk_packages"),
                github_ref="refs/heads/main",
                github_event_name="workflow_dispatch",
                github_event_inputs_publish_release_packages="true",
                github_event_inputs_registry_dry_run="true",
                github_event_inputs_release_tag="",
                needs_release_validate_result="success",
                needs_build_binaries_result="skipped",
                needs_publish_github_release_result="skipped",
            )
        )

    def test_the_sdk_is_gated_no_more_weakly_than_the_crate(self):
        # Parity, so the two publication jobs cannot drift apart again.
        sdk = job_condition("publish_sdk_packages")
        crate = job_condition("publish_registries")
        gate = (
            "((github.event_name == 'workflow_dispatch' && "
            "github.event.inputs.registry_dry_run == 'true') || "
            "(needs.build_binaries.result == 'success' && "
            "needs.publish_github_release.result == 'success'))"
        )
        self.assertIn(gate, sdk)
        self.assertIn(gate, crate)

    def test_the_gate_is_declared_in_needs_so_it_is_actually_waited_on(self):
        # An `if` naming a job absent from `needs` reads as '' and never blocks.
        block = job_block("publish_sdk_packages")
        self.assertIn(
            "needs: [release_validate, build_binaries, publish_github_release]", block
        )

    def test_always_is_retained_so_skipped_upstreams_do_not_skip_the_job(self):
        self.assertIn("always()", job_condition("publish_sdk_packages"))

    def test_the_bounded_slice_does_not_leak_the_next_job(self):
        # Guards the helper itself: an unbounded slice made these assertions
        # pass against unmodified code, because publish_registries carries the
        # same strings.
        block = job_block("publish_sdk_packages")
        self.assertIn("publish_sdk_packages:", block)
        self.assertNotIn("publish_registries:", block)
        self.assertNotIn("Publish Rust crate", block)


if __name__ == "__main__":
    unittest.main()
