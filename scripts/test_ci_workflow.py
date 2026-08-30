#!/usr/bin/env python3
"""Focused contracts for the pull-request CI workflow."""

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CI_WORKFLOW = ROOT / ".github/workflows/ci.yml"


def job_block(name: str) -> str:
    """Return exactly one top-level job, bounded by the next job header."""
    lines = CI_WORKFLOW.read_text().splitlines()
    start = lines.index(f"  {name}:")
    for offset, line in enumerate(lines[start + 1 :], start=start + 1):
        if line.startswith("  ") and not line.startswith("   ") and line.endswith(":"):
            return "\n".join(lines[start:offset])
    return "\n".join(lines[start:])


class CiWorkflowTests(unittest.TestCase):
    def test_workspace_nextest_preserves_all_failure_evidence(self):
        commands = [
            line.strip()
            for line in job_block("test").splitlines()
            if line.strip().startswith("- run: scripts/repo-cargo nextest run --workspace")
        ]

        self.assertEqual(len(commands), 1, commands)
        self.assertIn(" --no-fail-fast", commands[0])

    def test_decorator_authority_gate_actually_runs_in_ci(self):
        """A structural gate nobody invokes is a control that cannot fail.

        `verify-decorator-authority.py` exists because a decorator omitting
        `request_attempt_authority` compiles clean, warns nothing, and stranded
        72 identities downstream. If it is ever dropped from `fmt-lint` the
        defect class goes unwatched again with no other signal, so pin the
        invocation here rather than trusting that the file's presence means it
        runs.
        """
        block = job_block("fmt-lint")
        self.assertIn(
            "- run: python3 scripts/verify-decorator-authority.py",
            block,
        )

    def test_typescript_sdk_suite_actually_runs_in_ci(self):
        """705 TypeScript tests existed and gated nothing.

        No job ran `sdk/typescript` at all: the only Makefile reference is
        `publish-dry-run-typescript`, which builds and packs without testing.
        So a wrong RPC method name, or a parser reading camelCase where the
        gateway sends snake_case, reached npm behind a typecheck that cannot
        see either. Pin the invocation rather than the job's existence - a job
        that stops running its tests looks identical to one that passes.

        `validate` and not `test`: most of these tests import the built
        `dist/`, so `test` alone fails with ERR_MODULE_NOT_FOUND.
        """
        block = job_block("test-typescript")
        self.assertIn(
            "- run: npm --prefix sdk/typescript run validate --silent",
            block,
        )

    def test_gate_requires_every_job_it_lists(self):
        """A job absent from `gate` can fail without failing the check suite.

        `needs` alone does not gate: with `if: always()` the gate runs
        regardless, so a job missing from the result comparison is advisory
        only. Both lists must name the same jobs.
        """
        block = job_block("gate")
        needs = block.split("needs: [", 1)[1].split("]", 1)[0]
        listed = {name.strip() for name in needs.split(",")}
        for name in listed:
            self.assertIn(
                'needs.%s.result }}" != "success"' % name,
                block,
                f"{name} is in `needs` but its result is never compared, so it "
                f"cannot fail the gate",
            )



if __name__ == "__main__":
    unittest.main()
