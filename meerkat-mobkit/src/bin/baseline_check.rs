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
//! Phase 0 binary — verifies Meerkat baseline symbols before runtime startup.

use std::{process, time::Duration};

use meerkat_mobkit::run_meerkat_baseline_verification_once;

fn shell_escape_single_quotes(input: &str) -> String {
    input.replace('\'', "'\"'\"'")
}

fn main() {
    let repo_root =
        std::env::var("MEERKAT_REPO").unwrap_or_else(|_| "/Users/luka/src/raik".to_string());
    let escaped = shell_escape_single_quotes(&repo_root);
    let script = format!("printf '%s\\n' '{{\"repo_root\":\"{escaped}\"}}'");
    let args = vec!["-c".to_string(), script];

    let report =
        match run_meerkat_baseline_verification_once("sh", &args, &[], Duration::from_secs(5)) {
            Ok(report) => report,
            Err(err) => {
                eprintln!("baseline validation failed: {err:?}");
                process::exit(1);
            }
        };

    println!(
        "repo={} missing_symbols={}",
        repo_root,
        report.missing_symbols.len()
    );
}
