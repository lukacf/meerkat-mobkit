use std::path::PathBuf;
use std::process::Command;

use meerkat_mobkit_core::{
    validate_governance_state, validate_phase0_governance_contracts,
    validate_traceability_statuses, GovernanceValidationError,
};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("project root should resolve")
}

#[test]
fn phase0_governance_contracts_validate_current_repo_state() {
    let root = project_root();
    let spec = std::fs::read_to_string(root.join(".rct/spec.yaml")).expect("read spec");
    let plan = std::fs::read_to_string(root.join(".rct/plan.yaml")).expect("read plan");
    let checklist =
        std::fs::read_to_string(root.join(".rct/checklist.yaml")).expect("read checklist");
    let traceability =
        std::fs::read_to_string(root.join("docs/rct/traceability.md")).expect("read traceability");

    validate_phase0_governance_contracts(&spec, &plan, &checklist, &traceability)
        .expect("current governance contract should validate");
}

#[test]
fn phase0_governance_rejects_invalid_governance_state() {
    let err = validate_governance_state("spec", "governance_state: blocked")
        .expect_err("invalid governance state must fail");
    assert_eq!(
        err,
        GovernanceValidationError::InvalidGovernanceState {
            file: "spec".to_string(),
            found: "blocked".to_string(),
        }
    );
}

#[test]
fn phase0_governance_rejects_unknown_traceability_status() {
    let markdown = "\
| Trace-ID | Requirement ID | Phase | Evidence Log | Status |\n\
|----------|----------------|-------|--------------|--------|\n\
| TR-999   | REQ-X          | P0    | .rct/outputs/P0/ | PENDING |\n";
    let err = validate_traceability_statuses(markdown)
        .expect_err("unknown status should fail validation");
    assert!(matches!(
        err,
        GovernanceValidationError::InvalidTraceabilityStatus { .. }
    ));
}

#[test]
fn phase0_governance_binary_runs_against_repo_files() {
    let root = project_root();
    let output = Command::new(env!("CARGO_BIN_EXE_phase0_governance_check"))
        .current_dir(&root)
        .env("MOBKIT_ROOT", &root)
        .output()
        .expect("phase0 governance binary should run");
    assert!(
        output.status.success(),
        "phase0_governance_check failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
