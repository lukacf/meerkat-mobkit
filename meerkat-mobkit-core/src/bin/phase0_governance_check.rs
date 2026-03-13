#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::uninlined_format_args,
    clippy::collapsible_if,
    clippy::redundant_clone,
    clippy::needless_raw_string_hashes,
    clippy::single_match,
    clippy::redundant_closure_for_method_calls,
    clippy::redundant_pattern_matching,
    clippy::ignored_unit_patterns,
    clippy::clone_on_copy,
    clippy::manual_assert,
    clippy::unwrap_in_result,
    clippy::useless_vec
)]
//! Phase 0 binary — validates governance policies before runtime startup.

use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process,
};

use meerkat_mobkit_core::validate_phase0_governance_contracts;

fn read_required(path: &PathBuf) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("failed to read {}: {err}", path.display()))
}

fn read_traceability(root: &Path) -> Result<String, String> {
    let primary = root.join(".rct/traceability.md");
    match fs::read_to_string(&primary) {
        Ok(contents) => Ok(contents),
        Err(err) if err.kind() == ErrorKind::NotFound => {
            let fallback = root.join("docs/rct/traceability.md");
            fs::read_to_string(&fallback).map_err(|fallback_err| {
                format!(
                    "failed to read {} (missing) and {}: {fallback_err}",
                    primary.display(),
                    fallback.display(),
                )
            })
        }
        Err(err) => Err(format!("failed to read {}: {err}", primary.display())),
    }
}

fn project_root() -> PathBuf {
    if let Ok(root) = std::env::var("MOBKIT_ROOT") {
        return PathBuf::from(root);
    }
    PathBuf::from(".")
}

fn main() {
    let root = project_root();
    let spec_path = root.join(".rct/spec.yaml");
    let plan_path = root.join(".rct/plan.yaml");
    let checklist_path = root.join(".rct/checklist.yaml");

    let spec = match read_required(&spec_path) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    };
    let plan = match read_required(&plan_path) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    };
    let checklist = match read_required(&checklist_path) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    };
    let traceability = match read_traceability(&root) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("{err}");
            process::exit(1);
        }
    };

    if let Err(err) = validate_phase0_governance_contracts(&spec, &plan, &checklist, &traceability)
    {
        eprintln!("phase0 governance validation failed: {err}");
        process::exit(1);
    }

    println!("phase0 governance validation passed");
}
