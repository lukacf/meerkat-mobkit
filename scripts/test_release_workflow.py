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
    )
    # `!=` must survive as `!=`. A blanket `!` -> ` not ` rewrite turns it into
    # ` not =` and raises SyntaxError, so any condition using inequality could
    # not be evaluated at all - publish_docs is the only one that does, and it
    # was therefore untested until the CI-gate tests reached it.
    translated = re.sub(r"!(?!=)", " not ", translated)
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


def ci_gate_script() -> str:
    """The require_ci_green github-script body, extracted verbatim."""
    lines = RELEASE_WORKFLOW.read_text().splitlines()
    step = lines.index("      - name: Refuse unless exact-main CI is already green for this commit")
    start = lines.index("          script: |", step)
    body: list[str] = []
    for line in lines[start + 1 :]:
        if line and not line.startswith("            "):
            break
        body.append(line[12:] if line else "")
    return "\n".join(body)


def run_ci_gate(runs, release_sha="deadbeef", *, is_dispatch=False,
                registry_dry_run="", release_tag=""):
    """Execute the gate under node against a stubbed workflow-run listing.

    The gate's decision lives in JavaScript, so asserting on its source text
    cannot distinguish "requires success" from "requires completion" - exactly
    the mutation that a string assertion lets through. This runs the real body.
    """
    harness = """
const RUNS = %s;
const calls = [];
const core = {
  info() {},
  setFailed(message) { calls.push({ kind: "setFailed", message }); },
  setOutput(name, value) { calls.push({ kind: "setOutput", name, value }); },
};
const context = { repo: { owner: "lukacf", repo: "meerkat-mobkit" } };
const github = {
  rest: { actions: { listWorkflowRuns: async () => ({ data: { workflow_runs: RUNS } }) } },
};
process.env.RELEASE_SHA = %s;
process.env.IS_DISPATCH = %s;
process.env.REGISTRY_DRY_RUN = %s;
process.env.RELEASE_TAG_INPUT = %s;
(async () => {
%s
})().then(() => console.log(JSON.stringify(calls)));
""" % (
        json.dumps(runs),
        json.dumps(release_sha),
        json.dumps("true" if is_dispatch else "false"),
        json.dumps(registry_dry_run),
        json.dumps(release_tag),
        ci_gate_script(),
    )
    with tempfile.TemporaryDirectory() as tmp:
        script = Path(tmp) / "gate.mjs"
        script.write_text(harness)
        out = subprocess.run(
            ["node", str(script)], capture_output=True, text=True, check=True
        )
    return json.loads(out.stdout.strip().splitlines()[-1])


def workflow_permissions_without_comments() -> str:
    """The permissions block with comment lines removed.

    Asserting on raw text matched the explanatory comment above the key, so
    deleting `actions: read` left the test green. The prose is not the config.
    """
    head = RELEASE_WORKFLOW.read_text().split("jobs:", 1)[0]
    return "\n".join(
        line for line in head.splitlines() if not line.strip().startswith("#")
    )


PORTABILITY_SCRIPT = ROOT / "scripts/check-linux-release-binary-portability.sh"

# glibc shipped by each Debian release the container expression may name. The
# gate's default floor must equal the entry for the image in use; an image move
# to a codename missing here fails the parity test until the map is extended,
# which is the moment to decide the new floor deliberately.
CONTAINER_GLIBC = {"bullseye": "2.31", "bookworm": "2.36", "trixie": "2.41"}

# readelf --dyn-syms output shapes. GLIBC_2.4 (__stack_chk_fail) is in nearly
# every binary and sorts ABOVE "2.31" as text, below it as a version.
DYNSYMS_WITHIN_FLOOR = """\
Symbol table '.dynsym' contains 3 entries:
   Num:    Value          Size Type    Bind   Vis      Ndx Name
     1: 0000000000000000     0 FUNC    GLOBAL DEFAULT  UND memcpy@GLIBC_2.14 (2)
     2: 0000000000000000     0 FUNC    GLOBAL DEFAULT  UND __stack_chk_fail@GLIBC_2.4 (3)
     3: 0000000000000000     0 FUNC    GLOBAL DEFAULT  UND gettid@GLIBC_2.30 (4)
"""
# The v0.8.30 shape: std's pidfd spawn path bound to the runner's glibc 2.39.
DYNSYMS_ABOVE_FLOOR = DYNSYMS_WITHIN_FLOOR + (
    "     4: 0000000000000000     0 FUNC    WEAK   DEFAULT  UND pidfd_spawnp@GLIBC_2.39 (5)\n"
)
DYNAMIC_RUSTLS = """\
Dynamic section at offset 0x1000 contains 3 entries:
  Tag        Type                         Name/Value
 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]
 0x0000000000000001 (NEEDED)             Shared library: [libm.so.6]
 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]
"""
DYNAMIC_OPENSSL = DYNAMIC_RUSTLS + (
    " 0x0000000000000001 (NEEDED)             Shared library: [libssl.so.3]\n"
    " 0x0000000000000001 (NEEDED)             Shared library: [libcrypto.so.3]\n"
)


def step_block(step_name: str, job: str = "build_binaries") -> str:
    """The text of exactly one step, bounded by the next `- name:`."""
    lines = job_block(job).splitlines()
    start = lines.index(f"      - name: {step_name}")
    for offset, line in enumerate(lines[start + 1 :], start=start + 1):
        if line.startswith("      - name: "):
            return "\n".join(lines[start:offset])
    return "\n".join(lines[start:])


def step_script(step_name: str, job: str = "build_binaries") -> str:
    """One step's `run: |` body, de-indented, verbatim."""
    lines = job_block(job).splitlines()
    step = lines.index(f"      - name: {step_name}")
    run = lines.index("        run: |", step)
    body: list[str] = []
    for line in lines[run + 1 :]:
        if line and not line.startswith("          "):
            break
        body.append(line[10:] if line else "")
    return "\n".join(body)


def scalar(block: str, key: str, indent: str) -> str:
    """The raw value of the single `<indent><key>: value` line in a block."""
    matches = [line for line in block.splitlines() if line.startswith(f"{indent}{key}: ")]
    if len(matches) != 1:
        raise AssertionError(f"expected exactly one `{key}` line, found {len(matches)}")
    return matches[0].split(": ", 1)[1]


def matrix_targets(job: str = "build_binaries") -> list[str]:
    return re.findall(r"^\s+target: (\S+)$", job_block(job), re.M)


def evaluate_expression(expression: str, **context):
    """Evaluate a GitHub expression that yields a VALUE, under an explicit context.

    `evaluate_condition` collapses to bool. The `container:` image and the cache
    `key:` are strings built with the `cond && 'a' || 'b'` idiom, whose result
    is an operand, and Python's `and`/`or` return operands the same way. Any
    `matrix.*` name the caller did not supply raises rather than reading as ''.
    """
    translated = expression.replace("&&", " and ").replace("||", " or ")
    translated = re.sub(
        r"\bmatrix\.([A-Za-z_][A-Za-z0-9_-]*)",
        lambda m: "matrix_" + m.group(1).replace("-", "_"),
        translated,
    )
    translated = re.sub(r"contains\(([^,]+),\s*('[^']*')\)", r"(\2 in \1)", translated)
    missing = {
        name for name in re.findall(r"\bmatrix_[A-Za-z0-9_]+", translated) if name not in context
    }
    if missing:
        raise AssertionError(f"expression references unsupplied names: {sorted(missing)}")
    return eval(translated, {"__builtins__": {}}, dict(context))


def render_template(template: str, **context) -> str:
    """Substitute every `${{ expr }}` in a YAML scalar with its evaluated value."""
    return re.sub(
        r"\$\{\{\s*(.*?)\s*\}\}",
        lambda m: str(evaluate_expression(m.group(1), **context)),
        template,
    )


def run_portability_check(dynsyms: str, dynamic: str, *, floor: str | None = None,
                          binaries: list[str] | None = None) -> subprocess.CompletedProcess[str]:
    """Run the real gate against a fake `readelf`.

    The ELF the gate reads is the release artifact, which no test can build,
    and macOS has no readelf at all, so the readelf OUTPUT is the fixture: the
    script's decision is under test, not binutils. Only `rpc_gateway` exists
    on disk; other names in `binaries` exercise the missing-file path.
    """
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        bin_dir = root / "bin"
        bin_dir.mkdir()
        fake = bin_dir / "readelf"
        fake.write_text(
            """#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  --dyn-syms) printf '%s' "$FAKE_READELF_DYNSYMS" ;;
  -d) printf '%s' "$FAKE_READELF_DYNAMIC" ;;
  *) echo "fake readelf: unexpected arguments: $*" >&2; exit 99 ;;
esac
"""
        )
        fake.chmod(0o755)
        (root / "rpc_gateway").write_bytes(b"\x7fELF fixture")
        env = os.environ.copy()
        env["PATH"] = f"{bin_dir}:{env['PATH']}"
        env["FAKE_READELF_DYNSYMS"] = dynsyms
        env["FAKE_READELF_DYNAMIC"] = dynamic
        env.pop("MOBKIT_GLIBC_FLOOR", None)
        if floor is not None:
            env["MOBKIT_GLIBC_FLOOR"] = floor
        names = ["rpc_gateway"] if binaries is None else binaries
        return subprocess.run(
            [str(PORTABILITY_SCRIPT), *(str(root / name) for name in names)],
            env=env,
            capture_output=True,
            text=True,
        )


def run_portability_step(target: str, *, gate_status: int) -> tuple[subprocess.CompletedProcess[str], list[str]]:
    """Execute the `Check Linux binary portability` step body under bash.

    A fake gate script records what it was asked to check and exits with
    `gate_status`, so the test observes which binaries the step covers and
    whether a refusal propagates, rather than pattern-matching the YAML.
    """
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        scripts = root / "scripts"
        scripts.mkdir()
        log = root / "calls.log"
        fake = scripts / "check-linux-release-binary-portability.sh"
        fake.write_text(
            '#!/usr/bin/env bash\nprintf "%s\\n" "$@" >> "$FAKE_GATE_LOG"\nexit "$FAKE_GATE_STATUS"\n'
        )
        fake.chmod(0o755)
        env = os.environ.copy()
        env.update({"TARGET": target, "FAKE_GATE_LOG": str(log), "FAKE_GATE_STATUS": str(gate_status)})
        result = subprocess.run(
            ["bash", "-c", step_script("Check Linux binary portability")],
            cwd=root,
            env=env,
            capture_output=True,
            text=True,
        )
        checked = log.read_text().splitlines() if log.exists() else []
    return result, checked


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


class ExactMainCiGateTests(unittest.TestCase):
    """The release path must refuse a commit that was never green on main.

    Before `require_ci_green`, `release_validate` had no `needs` at all, so a
    tag published without anyone checking the commit. v0.8.26 was gated by hand.
    A hand check is not a control: it is present exactly when someone remembers.

    These evaluate the real `if:` expressions under a context where the gate
    failed, rather than asserting that the string "require_ci_green" appears
    somewhere. A job can name the gate in `needs` and still run anyway - three
    of these jobs carry `always()`, which is precisely what makes `needs` stop
    being a gate on its own.
    """

    # A gate failure SKIPS release_validate; it does not fail it. Every guard
    # downstream is written against 'success', so 'skipped' is the value that
    # actually occurs and the one worth testing.
    GATE_FAILED = dict(
        github_ref="refs/tags/v0.8.27",
        github_event_name="push",
        github_event_inputs_publish_release_packages="",
        github_event_inputs_registry_dry_run="",
        github_event_inputs_release_tag="",
        needs_release_validate_result="skipped",
        needs_build_binaries_result="skipped",
        needs_publish_github_release_result="skipped",
        needs_publish_registries_result="skipped",
        needs_publish_sdk_packages_result="skipped",
    )

    def test_no_publishing_job_runs_when_the_ci_gate_did_not_pass(self):
        for job in (
            "publish_sdk_packages",
            "publish_registries",
            "publish_docs",
        ):
            with self.subTest(job=job):
                self.assertFalse(
                    evaluate_condition(job_condition(job), **self.GATE_FAILED)
                )

    def test_the_dry_run_lane_cannot_bypass_the_ci_gate(self):
        # publish_sdk_packages and publish_registries carry a dry-run disjunct
        # that deliberately tolerates skipped binaries. That disjunct must not
        # also tolerate a skipped CI gate, or a dispatch would route around it.
        for job in ("publish_sdk_packages", "publish_registries"):
            with self.subTest(job=job):
                self.assertFalse(
                    evaluate_condition(
                        job_condition(job),
                        github_ref="refs/heads/main",
                        github_event_name="workflow_dispatch",
                        github_event_inputs_publish_release_packages="true",
                        github_event_inputs_registry_dry_run="true",
                        github_event_inputs_release_tag="",
                        needs_release_validate_result="skipped",
                        needs_build_binaries_result="skipped",
                        needs_publish_github_release_result="skipped",
                        needs_publish_sdk_packages_result="skipped",
                    )
                )

    def test_release_validate_waits_on_the_gate_rather_than_merely_coexisting(self):
        block = job_block("release_validate")
        self.assertIn("needs: [require_ci_green]", block)

    def test_the_gate_refuses_rather_than_polls(self):
        # A poll converts "this commit was never verified" into "this is taking
        # a while", and burns the release budget on a precondition that should
        # already hold. If someone reintroduces waiting, this should fail.
        block = job_block("require_ci_green")
        self.assertIn("Refuse unless exact-main CI is already green", block)
        self.assertNotIn("setTimeout", block)

    def test_the_gate_can_read_workflow_runs(self):
        # A `permissions:` block sets every unlisted scope to none, so without
        # `actions: read` the gate fails on an API 403 - a refusal for a reason
        # unrelated to CI, which reads identical to a real red at a glance.
        self.assertIn("actions: read", workflow_permissions_without_comments())

    def test_a_green_exact_main_run_is_accepted(self):
        calls = run_ci_gate(
            [{"head_sha": "deadbeef", "head_branch": "main", "event": "push",
              "status": "completed", "conclusion": "success",
              "run_attempt": 1, "id": 42, "html_url": "u"}]
        )
        self.assertEqual([c["kind"] for c in calls].count("setFailed"), 0)
        self.assertIn("setOutput", [c["kind"] for c in calls])

    def test_a_completed_but_failed_run_is_refused(self):
        # THE assertion of the whole gate. Dropping `conclusion === "success"`
        # leaves a check that accepts any terminal state, including failure.
        calls = run_ci_gate(
            [{"head_sha": "deadbeef", "head_branch": "main", "event": "push",
              "status": "completed", "conclusion": "failure",
              "run_attempt": 1, "id": 42, "html_url": "u"}]
        )
        self.assertEqual([c["kind"] for c in calls], ["setFailed"])

    def test_a_commit_with_no_exact_main_run_is_refused(self):
        self.assertEqual([c["kind"] for c in run_ci_gate([])], ["setFailed"])

    def test_a_pull_request_run_on_the_same_sha_is_not_accepted(self):
        # The v0.8.26 shape: the PR head was green on pull_request and had zero
        # push-to-main runs. A merge-result run answers a different question.
        calls = run_ci_gate(
            [{"head_sha": "deadbeef", "head_branch": "codex/x", "event": "pull_request",
              "status": "completed", "conclusion": "success",
              "run_attempt": 1, "id": 42, "html_url": "u"}]
        )
        self.assertEqual([c["kind"] for c in calls], ["setFailed"])

    def test_a_pure_dry_run_dispatch_is_not_gated(self):
        # Nothing is published on this lane, so there is no publication to
        # verify. Gating it would skip release_validate and silently retire
        # `python -m build`, `twine check` and `npm publish --dry-run` - the
        # exact three checks #340 kept `always()` in order to preserve.
        calls = run_ci_gate(
            [], is_dispatch=True, registry_dry_run="true", release_tag=""
        )
        self.assertEqual(calls, [])

    def test_the_dry_run_carve_out_does_not_cover_a_real_asset_dispatch(self):
        # release_tag set means publish_github_release uploads real assets even
        # with registry_dry_run on. That is a publication and must be gated.
        calls = run_ci_gate(
            [], is_dispatch=True, registry_dry_run="true", release_tag="v0.8.27"
        )
        self.assertEqual([c["kind"] for c in calls], ["setFailed"])

    def test_the_dry_run_carve_out_does_not_cover_a_real_registry_dispatch(self):
        calls = run_ci_gate(
            [], is_dispatch=True, registry_dry_run="false", release_tag=""
        )
        self.assertEqual([c["kind"] for c in calls], ["setFailed"])

    def test_the_dry_run_carve_out_does_not_cover_a_tag_push(self):
        # A tag push is never a dispatch, so the carve-out must not reach it
        # even if the dry-run input is somehow populated.
        calls = run_ci_gate(
            [], is_dispatch=False, registry_dry_run="true", release_tag=""
        )
        self.assertEqual([c["kind"] for c in calls], ["setFailed"])

    def test_an_in_flight_run_is_refused_rather_than_awaited(self):
        calls = run_ci_gate(
            [{"head_sha": "deadbeef", "head_branch": "main", "event": "push",
              "status": "in_progress", "conclusion": None,
              "run_attempt": 1, "id": 42, "html_url": "u"}]
        )
        self.assertEqual([c["kind"] for c in calls], ["setFailed"])

    def test_the_gate_requires_main_and_push_not_merely_the_sha(self):
        # A pull_request run on the same SHA is a different question: it tests
        # the merge result, not the commit as it lands on main.
        block = job_block("require_ci_green")
        self.assertIn('run.head_branch === "main"', block)
        self.assertIn('run.event === "push"', block)


class LinuxBinaryPortabilityGateTests(unittest.TestCase):
    """The gate reads the produced binary and refuses one the floor cannot load.

    A Linux GNU binary's glibc floor is whatever glibc it was linked against,
    and until this gate nothing read the result: the v0.8.30 gateways, built
    directly on ubuntu-latest, carried a hard GLIBC_2.39 version requirement
    (pidfd_spawnp/pidfd_getpid) and the loader refused them on Debian bookworm
    (2.36), Ubuntu 22.04 (2.35) and bullseye (2.31). Every job read green.
    """

    def test_a_glibc_reference_above_the_floor_is_refused(self):
        result = run_portability_check(DYNSYMS_ABOVE_FLOOR, DYNAMIC_RUSTLS)

        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn(
            "references GLIBC_2.39, above the declared floor GLIBC_2.31", result.stderr
        )

    def test_a_binary_within_the_floor_passes_and_names_its_maximum(self):
        result = run_portability_check(DYNSYMS_WITHIN_FLOOR, DYNAMIC_RUSTLS)

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("PASS", result.stdout)
        # The input also carries GLIBC_2.4, which a string sort would report as
        # the maximum and then compare above 2.31. Only a version sort names 2.30.
        self.assertIn("max glibc ref GLIBC_2.30, floor GLIBC_2.31", result.stdout)

    def test_the_floor_is_the_declared_parameter_not_a_constant(self):
        result = run_portability_check(DYNSYMS_ABOVE_FLOOR, DYNAMIC_RUSTLS, floor="2.39")

        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertIn("floor GLIBC_2.39", result.stdout)

    def test_a_dynamic_openssl_dependency_is_refused(self):
        # All first-party TLS is rustls; a NEEDED libssl means a dependency
        # regressed into native-tls, as meerkat v0.8.21 shipped via oai-rt-rs.
        result = run_portability_check(DYNSYMS_WITHIN_FLOOR, DYNAMIC_OPENSSL)

        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("dynamically links OpenSSL", result.stderr)
        self.assertIn("libssl.so.3", result.stderr)

    def test_one_missing_binary_fails_the_whole_set(self):
        result = run_portability_check(
            DYNSYMS_WITHIN_FLOOR, DYNAMIC_RUSTLS, binaries=["rpc_gateway", "mobkit_gateway"]
        )

        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("PASS", result.stdout)
        self.assertIn("mobkit_gateway: file not found", result.stderr)

    def test_the_gate_is_executable_from_a_checkout(self):
        # The workflow invokes it as ./scripts/...; a 644 file fails the step
        # with "Permission denied", which reads as a broken gate, not a floor.
        self.assertTrue(os.access(PORTABILITY_SCRIPT, os.X_OK))
        self.assertTrue(PORTABILITY_SCRIPT.read_text().startswith("#!/usr/bin/env bash\n"))


class LinuxReleaseContainerTests(unittest.TestCase):
    """Linux GNU release legs build inside a pinned-glibc container and every
    produced binary passes the portability gate before it is packaged.

    These evaluate the real `container:` and cache `key:` expressions per
    matrix target and execute the gate step's shell, rather than asserting
    that the word "bullseye" appears somewhere in the file.
    """

    def container_expression(self) -> str:
        return scalar(job_block("build_binaries"), "container", "    ")

    def linux_image(self) -> str:
        return render_template(self.container_expression(), matrix_target="x86_64-unknown-linux-gnu")

    def test_linux_gnu_legs_build_inside_the_pinned_container_and_others_do_not(self):
        targets = matrix_targets()
        self.assertEqual(len(targets), 5, targets)
        for target in targets:
            with self.subTest(target=target):
                image = render_template(self.container_expression(), matrix_target=target)
                if target.endswith("-unknown-linux-gnu"):
                    self.assertEqual(image, "docker.io/library/buildpack-deps:bullseye")
                else:
                    # macOS and Windows legs cannot run a Linux container; an
                    # empty string is how the expression opts them out.
                    self.assertEqual(image, "")

    def test_the_declared_floor_matches_the_container_image(self):
        # The floor is a fact about the image; the two are declared in two
        # files and this is the only thing that holds them together.
        codename = self.linux_image().rsplit(":", 1)[1]
        self.assertIn(
            codename, CONTAINER_GLIBC,
            f"unknown Debian codename {codename!r}: extend CONTAINER_GLIBC and decide the floor",
        )
        declared = re.search(
            r'^MOBKIT_GLIBC_FLOOR="\$\{MOBKIT_GLIBC_FLOOR:-([0-9.]+)\}"$',
            PORTABILITY_SCRIPT.read_text(),
            re.M,
        )
        self.assertIsNotNone(declared, "the gate must declare an overridable default floor")
        self.assertEqual(declared.group(1), CONTAINER_GLIBC[codename])

    def test_the_linux_cache_key_names_the_container(self):
        # rust-cache keys on rustc, the lock and the env; none changed when the
        # Linux legs moved into the container, so the host-built target dir
        # would be restored into it and the gate would refuse the same stale
        # cache on every retry. The other legs keep their existing keys.
        key = scalar(step_block("Cache cargo registry"), "key", "          ")
        linux = render_template(key, matrix_target="x86_64-unknown-linux-gnu")
        mac = render_template(key, matrix_target="aarch64-apple-darwin")

        self.assertIn(self.linux_image().rsplit(":", 1)[1], linux)
        self.assertEqual(mac, "release-aarch64-apple-darwin")

    def test_the_gate_runs_on_linux_after_the_build_and_before_anything_ships(self):
        block = job_block("build_binaries")
        gate = block.index("      - name: Check Linux binary portability")

        self.assertLess(block.index("      - name: Build gateway binaries"), gate)
        for later in ("Package artifacts", "Attest build provenance", "Upload artifacts"):
            with self.subTest(step=later):
                self.assertLess(gate, block.index(f"      - name: {later}"))
        step = step_block("Check Linux binary portability")
        self.assertIn("        if: runner.os == 'Linux'", step)
        self.assertIn("          TARGET: ${{ matrix.target }}", step)

    def test_the_gate_checks_every_binary_the_build_step_produces(self):
        built = re.findall(r"--bin (\w+)", step_script("Build gateway binaries"))
        self.assertTrue(built, "the build step names no --bin; the parser is wrong")
        target = "aarch64-unknown-linux-gnu"

        result, checked = run_portability_step(target, gate_status=0)

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(
            sorted(checked), sorted(f"target/{target}/release/{name}" for name in built)
        )

    def test_a_refused_binary_fails_the_step(self):
        result, checked = run_portability_step("x86_64-unknown-linux-gnu", gate_status=1)

        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertTrue(checked, "the gate was never invoked")


if __name__ == "__main__":
    unittest.main()
