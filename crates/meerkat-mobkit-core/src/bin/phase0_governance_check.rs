use std::{fs, path::PathBuf, process};

use meerkat_mobkit_core::validate_phase0_governance_contracts;

fn read_required(path: &PathBuf) -> Result<String, String> {
    fs::read_to_string(path).map_err(|err| format!("failed to read {}: {err}", path.display()))
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
    let traceability_path = root.join("docs/rct/traceability.md");

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
    let traceability = match read_required(&traceability_path) {
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
