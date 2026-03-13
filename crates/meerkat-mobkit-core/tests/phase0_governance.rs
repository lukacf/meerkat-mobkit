use std::path::{Path, PathBuf};
use std::process::Command;

use meerkat_mobkit_core::{
    GovernanceValidationError, validate_governance_state, validate_phase0_governance_contracts,
    validate_traceability_statuses,
};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("project root should resolve")
}

fn read_traceability_from_repo(root: &Path) -> String {
    let primary = root.join(".rct/traceability.md");
    match std::fs::read_to_string(&primary) {
        Ok(contents) => contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            let fallback = root.join("docs/rct/traceability.md");
            std::fs::read_to_string(&fallback).unwrap_or_else(|fallback_err| {
                panic!(
                    "traceability read failed for {} and {}: {fallback_err}",
                    primary.display(),
                    fallback.display()
                )
            })
        }
        Err(err) => panic!("traceability read failed for {}: {err}", primary.display()),
    }
}

#[test]
fn phase0_governance_contracts_validate_current_repo_state() {
    let root = project_root();
    let spec = std::fs::read_to_string(root.join(".rct/spec.yaml")).expect("read spec");
    let plan = std::fs::read_to_string(root.join(".rct/plan.yaml")).expect("read plan");
    let checklist =
        std::fs::read_to_string(root.join(".rct/checklist.yaml")).expect("read checklist");
    let traceability = read_traceability_from_repo(&root);

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
| REQ-ID | Phase | Implemented In | Runtime Caller | Evidence | Status |\n\
|--------|-------|----------------|----------------|----------|--------|\n\
| CONTRACT-999 | P0 | - | - | .rct/outputs/P0/ | PENDING |\n";
    let err = validate_traceability_statuses(markdown)
        .expect_err("unknown status should fail validation");
    assert!(matches!(
        err,
        GovernanceValidationError::InvalidTraceabilityStatus { .. }
    ));
}

#[test]
fn phase0_governance_accepts_all_current_traceability_statuses() {
    let mut markdown = String::from(
        "\
| REQ-ID | Phase | Implemented In | Runtime Caller | Evidence | Status |\n\
|--------|-------|----------------|----------------|----------|--------|\n",
    );
    for (index, status) in [
        "TYPED",
        "WIRED",
        "VALIDATED",
        "PROVISIONAL",
        "MISSING",
        "DEFERRED",
        "STUBBED",
    ]
    .iter()
    .enumerate()
    {
        markdown.push_str(&format!(
            "| TYPE-{index:03} | P0 | .rct/* | - | .rct/outputs/P0/ | {status} |\n"
        ));
    }

    validate_traceability_statuses(&markdown).expect("all governance statuses should be accepted");
}

#[test]
fn phase0_governance_rejects_missing_traceability_evidence() {
    let markdown = "\
| REQ-ID | Phase | Implemented In | Runtime Caller | Evidence | Status |\n\
|--------|-------|----------------|----------------|----------|--------|\n\
| TYPE-001 | P0 | .rct/* | - |   | TYPED |\n";
    let err = validate_traceability_statuses(markdown)
        .expect_err("missing evidence should fail validation");
    assert!(matches!(
        err,
        GovernanceValidationError::MissingTraceabilityEvidence { .. }
    ));
}

#[test]
fn phase0_governance_rejects_placeholder_traceability_evidence() {
    let markdown = "\
| REQ-ID | Phase | Implemented In | Runtime Caller | Evidence | Status |\n\
|--------|-------|----------------|----------------|----------|--------|\n\
| TYPE-001 | P0 | .rct/* | - | - | TYPED |\n";
    let err = validate_traceability_statuses(markdown)
        .expect_err("placeholder evidence should fail validation");
    assert!(matches!(
        err,
        GovernanceValidationError::MissingTraceabilityEvidence { .. }
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
