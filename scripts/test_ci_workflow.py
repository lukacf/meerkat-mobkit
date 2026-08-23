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


if __name__ == "__main__":
    unittest.main()
