//! End-to-end plumbing test for the calibration harness's live-mode seam
//! (docs/design/agent-memory-architecture.md §11, P1.3): `scripts/memory-evals
//! --stage selector --mode live` driving the real `selector-eval` binary in
//! `--mock` mode. MEMORY_EVALS_FORCE_MOCK=1 pins the deterministic lane so
//! this never attempts (or pays for) a live model call, regardless of what
//! auth the host machine could resolve.

use std::path::Path;
use std::process::Command;

#[test]
fn memory_evals_live_mode_runs_selector_eval_mock_end_to_end() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../scripts/memory-evals");
    if !script.is_file() {
        // Packaged crate builds have no repo scripts tree; nothing to test.
        return;
    }
    let selector_eval = env!("CARGO_BIN_EXE_selector-eval");

    let output = Command::new(&script)
        .args(["--stage", "selector", "--mode", "live"])
        .env("MEMORY_EVALS_SELECTOR_EVAL", selector_eval)
        .env("MEMORY_EVALS_FORCE_MOCK", "1")
        .output();
    let output = match output {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            // No python3 on this host; the harness itself is exercised in CI.
            return;
        }
        Err(err) => panic!("failed to run scripts/memory-evals: {err}"),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "mock-lane live mode must exit 0\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("forced to --mock"),
        "force-mock notice missing:\n{stdout}"
    );
    assert!(
        stdout.contains("MOCK-fallback selector scorecard"),
        "scorecard header missing:\n{stdout}"
    );
    assert!(
        stdout.contains("obvious-match"),
        "fixtures did not run:\n{stdout}"
    );
    assert!(
        !stdout.contains("UNSTABLE under shuffle"),
        "the deterministic mock must be shuffle-stable (§11):\n{stdout}"
    );
    assert!(
        stdout.contains("informational only, exit 0"),
        "mock lane must be non-gating:\n{stdout}"
    );
}
