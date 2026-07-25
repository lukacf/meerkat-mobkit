//! Head-canonical continuity DOWNGRADE: the rollback path for the one-way
//! `mobkit-continuity` v1 → v2 ledger bump, behind
//! `mobkit_gateway storage-downgrade`.
//!
//! # Why this verb exists
//!
//! Incremental continuity persistence (M4b) makes head+rows the canonical
//! durable representation of a session and stamps the file's
//! `mobkit-continuity` ledger domain at v2. That stamp is deliberately
//! one-way: a binary that does not understand head rows must NOT be allowed
//! to keep writing `session_snapshots.data` while head rows are the byte
//! authority, so every older release refuses the file at open with
//! `SchemaFromTheFuture`.
//!
//! The stamp is not applied by merely launching the new release — it rides
//! the first delta write that actually creates head state (see
//! `LocalContinuityStore::delta_write`). But once the capability is
//! advertised, EVERY boundary save routes through the incremental branch, so
//! in practice the first turn stamps the file. That made the effective
//! rollback window exactly one turn, and the only documented recovery
//! "restore `continuity.*` from a backup taken before that turn" — which
//! means discarding every turn taken since.
//!
//! This verb replaces that with a real downgrade: re-materialize every
//! head+rows session back into a whole-document `session_snapshots` blob,
//! drop the head-canonical trio, and rewind the ledger row to v1. The
//! post-upgrade turns are kept, and the previous release can open the file
//! again.
//!
//! # Shape
//!
//! Mirrors [`crate::storage_migrate`]: one blocking library function over a
//! state directory, run under the same [`MobKitMaintenanceFence`] (MobKit
//! builds no second fence), dry-run by default, JSON- or text-renderable
//! report, nonzero exit on refusals. The per-database transactional work is
//! [`LocalContinuityStore::downgrade_head_canonical_at`]; this module owns
//! locating the database, fencing it, and reporting.
//!
//! # What "dry run" means here
//!
//! Not an estimate. A dry run performs the ENTIRE reconstruction —
//! including the per-document reader simulation that decides fidelity — and
//! then rolls the transaction back. So a clean dry run is evidence the apply
//! run will succeed, not a guess that it might.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::identity_first::{DowngradeFidelity, HeadCanonicalDowngrade, LocalContinuityStore};
use crate::storage_layout::MobKitStorageLayout;
use crate::storage_migrate::{MigrateMode, MobKitMaintenanceFence};

/// How long an apply run waits for in-flight store operations to drain
/// before reporting the fence as unavailable. Same budget the migrate pass
/// uses.
const FENCE_DRAIN_DEADLINE: Duration = Duration::from_secs(10);

/// The full `storage-downgrade` report for one state directory.
#[non_exhaustive]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MobKitDowngradeReport {
    /// Dry-run or apply.
    #[serde(default)]
    pub mode: MigrateMode,
    /// The state directory operated on.
    #[serde(default)]
    pub state_dir: PathBuf,
    /// The continuity database resolved for this state dir, when one exists.
    #[serde(default)]
    pub continuity_db: Option<PathBuf>,
    /// Databases the fence covers (in apply mode, the fence actually held).
    #[serde(default)]
    pub fenced_databases: Vec<PathBuf>,
    /// The per-database transactional outcome, when the pass got that far.
    #[serde(default)]
    pub downgrade: Option<HeadCanonicalDowngrade>,
    /// Refusals and failures. Non-empty means the file was NOT downgraded.
    #[serde(default)]
    pub errors: Vec<String>,
}

impl MobKitDowngradeReport {
    fn new(mode: MigrateMode, state_dir: &Path) -> Self {
        Self {
            mode,
            state_dir: state_dir.to_path_buf(),
            ..Self::default()
        }
    }

    /// Whether the pass refused or failed.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Sessions whose retained rewrite history could not be re-inlined into
    /// a document a reader accepts. Their turn content is intact; their
    /// branch/rewind history is not. Reported loudly rather than treated as
    /// a failure, because refusing the downgrade over it would leave the
    /// operator with a file no previous release can open at all.
    #[must_use]
    pub fn lossy_session_count(&self) -> usize {
        self.downgrade
            .as_ref()
            .map(|downgrade| downgrade.lossy_sessions().len())
            .unwrap_or(0)
    }

    /// Whether the run left (or, in dry run, would leave) the continuity
    /// file openable by a pre-head-canonical binary.
    #[must_use]
    pub fn lockout_lifted(&self) -> bool {
        self.downgrade
            .as_ref()
            .is_some_and(HeadCanonicalDowngrade::lockout_lifted)
    }
}

/// Undo the head-canonical continuity upgrade for one MobKit state
/// directory.
///
/// Blocking (async callers should wrap it in `spawn_blocking`); the caller
/// renders the report and maps [`MobKitDowngradeReport::has_errors`] to a
/// nonzero exit.
///
/// Fail-closed at every step: an unresolvable layout, an unacquirable
/// fence, a ledger ahead of this binary, or head rows without the v2 stamp
/// all abort the pass with the file untouched. The downgrade itself is one
/// transaction per database, with the ledger rewind as its last statement,
/// so a crash mid-pass leaves the file exactly as head-canonical as it was.
#[must_use]
pub fn downgrade_state_dir(state_dir: &Path, mode: MigrateMode) -> MobKitDowngradeReport {
    let mut report = MobKitDowngradeReport::new(mode, state_dir);
    if !state_dir.is_dir() {
        report.errors.push(format!(
            "state directory {} does not exist",
            state_dir.display()
        ));
        return report;
    }
    let apply = mode == MigrateMode::Apply;
    let layout = MobKitStorageLayout::with_injected_roots(state_dir.to_path_buf(), None);

    let resolved = match layout.continuity_db() {
        Ok(resolved) => resolved,
        Err(error) => {
            report
                .errors
                .push(format!("continuity locator unresolved: {error}"));
            return report;
        }
    };
    if !resolved.path.is_file() {
        report.errors.push(format!(
            "no continuity database at {} — nothing to downgrade",
            resolved.path.display()
        ));
        return report;
    }
    report.continuity_db = Some(resolved.path.clone());

    // The fence covers the whole state directory, not just the continuity
    // file: a downgrade that ran while a gateway held other stores open
    // would be racing the process that is about to be rolled back.
    let fence = if apply {
        match MobKitMaintenanceFence::acquire(state_dir, FENCE_DRAIN_DEADLINE) {
            Ok(fence) => {
                report.fenced_databases = fence.fenced_databases().to_vec();
                Some(fence)
            }
            Err(error) => {
                report
                    .errors
                    .push(format!("maintenance fence not acquirable: {error}"));
                return report;
            }
        }
    } else {
        None
    };

    match LocalContinuityStore::downgrade_head_canonical_at(&resolved.path, apply) {
        Ok(downgrade) => report.downgrade = Some(downgrade),
        Err(error) => report.errors.push(format!(
            "continuity head-canonical downgrade failed: {error}"
        )),
    }
    drop(fence);
    report
}

/// Human-readable rendering of a downgrade report.
pub fn render_downgrade_report(report: &MobKitDowngradeReport) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let mode = match report.mode {
        MigrateMode::Apply => "apply",
        _ => "dry-run",
    };
    let _ = writeln!(
        out,
        "Continuity head-canonical downgrade ({mode}) over {}:",
        report.state_dir.display()
    );
    if let Some(path) = &report.continuity_db {
        let _ = writeln!(out, "  continuity database: {}", path.display());
    }
    match &report.downgrade {
        Some(downgrade) => {
            let _ = writeln!(
                out,
                "  ledger mobkit-continuity: {:?} -> {:?}",
                downgrade.ledger_before, downgrade.ledger_after
            );
            let _ = writeln!(
                out,
                "  head-canonical channel dropped: {}",
                downgrade.channel_dropped
            );
            let _ = writeln!(
                out,
                "  sessions re-materialized into whole-document snapshots: {}",
                downgrade.sessions.len()
            );
            for session in &downgrade.sessions {
                let fidelity = match &session.fidelity {
                    DowngradeFidelity::Full => "history preserved".to_string(),
                    DowngradeFidelity::NoHistory => "no history to preserve".to_string(),
                    DowngradeFidelity::HistoryDropped { reason } => {
                        format!("HISTORY DROPPED: {reason}")
                    }
                };
                let _ = writeln!(
                    out,
                    "    {} [{}] {} message(s), {} rewrite(s), {} bytes — {fidelity}",
                    session.session_id,
                    session.identity,
                    session.messages,
                    session.rewrites,
                    session.bytes
                );
            }
            let lossy = downgrade.lossy_sessions();
            if !lossy.is_empty() {
                let _ = writeln!(
                    out,
                    "  WARNING: {} session(s) lost their retained transcript rewrite history. \
                     Turn content is intact; branch/rewind history for those sessions is not.",
                    lossy.len()
                );
            }
            if downgrade.lockout_lifted() {
                let _ = writeln!(
                    out,
                    "  previous releases can open this file{}",
                    if downgrade.applied {
                        ""
                    } else {
                        " (after --apply; this run rolled back)"
                    }
                );
            }
        }
        None => {
            let _ = writeln!(out, "  no downgrade performed");
        }
    }
    for error in &report.errors {
        let _ = writeln!(out, "  error: {error}");
    }
    out
}
