//! Supported operator repair for mob-member durable transcripts (task #63):
//! drop superseded System rows from a session's durable transcript through
//! the typed rewrite door - one full-range audited rewrite commit composed
//! onto the retained graph, never a hand edit of store rows.
//!
//! Selection modes:
//! - DEFAULT: content-equality dedup - group System rows by
//!   (content, identity), keep the FIRST occurrence of each distinct group,
//!   drop the rest. Compares the system CONTENT, never the serialized
//!   envelope: each materialized copy carries its own `created_at`
//!   (verified in the field: 12 differing bytes, all inside the timestamp),
//!   so envelope hashing sees every copy as distinct.
//! - `--keep-newest <N>`: recency pruning - keep only the LAST N System
//!   rows by position, drop every earlier one. Strictly more destructive
//!   (discards genuinely different superseded prompt versions); an explicit
//!   operator lever, never the default.
//!
//! Procedure (single-writer discipline, unchanged from the field-proven
//! window-4/5 runs):
//!   1. STOP the gateway.
//!   2. Back up continuity.db (byte copy).
//!   3. Dry run:  mobkit-repair --db /path/to/continuity.db --all-sessions
//!   4. Apply:    ... --apply
//!   5. Remove the runtime scratch store (runtime.db file set) so the next
//!      boot mints runtime authority from the healed durable rows (the
//!      fleet-proven reset-reseed lane).
//!   6. Start the gateway.
//!
//! Fleet passes commit PER SESSION atomically: a typed refusal on one
//! session is recorded in the report and the pass continues, so a refusal
//! on member 7 never strands members 8-15 in an unknown state.
//!
//! Output: one JSON report document on stdout (and to `--report <path>` if
//! given); human progress goes to stderr. Exit code is 0 when every
//! selected session ended in a non-refused outcome, 1 otherwise.
//!
//! Byte semantics (field-misread twice, so stated here and in the report):
//! `bytes_before`/`bytes_after` measure the live HEAD transcript - the
//! strand a resume actually serves and the provider actually reads - NOT
//! the on-disk file. Audited rewrites RETAIN prior strands by design, so
//! the store file does not shrink after a successful apply; the head does.
//! This is a head-size repair (provider-input pressure), not a disk-space
//! repair.

use std::sync::Arc;

use meerkat_mobkit::identity_first::{
    ContinuitySessionStoreAdapter, ContinuityStore, LocalContinuityStore, SessionRuntimeState,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SelectionMode {
    ContentDedup,
    KeepNewest(usize),
}

#[derive(Debug, serde::Serialize)]
struct SessionReport {
    session_id: String,
    identity: Option<String>,
    outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
    rows_before: Option<usize>,
    rows_after: Option<usize>,
    bytes_before: Option<usize>,
    bytes_after: Option<usize>,
    system_rows_before: Option<usize>,
    system_rows_after: Option<usize>,
}

#[derive(Debug, serde::Serialize)]
struct RepairReport {
    mode: String,
    applied: bool,
    sessions: Vec<SessionReport>,
    refused: usize,
    /// Constant clarifier shipped IN the report because it was misread twice
    /// in the field: bytes measure the healed HEAD strand, not the file.
    bytes_semantics: &'static str,
}

const BYTES_SEMANTICS: &str = "bytes_before/bytes_after measure the live HEAD transcript (what \
     a resume serves and the provider reads), not the on-disk file: audited rewrites retain \
     prior strands by design, so the store file does not shrink after apply";

#[tokio::main(flavor = "current_thread")]
async fn main() {
    match run().await {
        Ok(exit) => std::process::exit(exit),
        Err(error) => {
            eprintln!("mobkit-repair: {error}");
            std::process::exit(2);
        }
    }
}

async fn run() -> Result<i32, Box<dyn std::error::Error>> {
    let mut db: Option<String> = None;
    let mut sessions: Vec<String> = Vec::new();
    let mut all_sessions = false;
    let mut keep_newest: Option<usize> = None;
    let mut apply = false;
    let mut report_path: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = args.next(),
            "--session" => {
                sessions.push(args.next().ok_or("--session requires a value")?);
            }
            "--all-sessions" => all_sessions = true,
            "--keep-newest" => {
                let value = args.next().ok_or("--keep-newest requires a value")?;
                let n: usize = value
                    .parse()
                    .map_err(|_| format!("--keep-newest must be a number: {value}"))?;
                if n == 0 {
                    return Err("--keep-newest must be >= 1 (a session keeps at least \
                                its newest System row)"
                        .into());
                }
                keep_newest = Some(n);
            }
            "--apply" => apply = true,
            "--report" => report_path = args.next(),
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }
    let db = db.ok_or("--db <path to the continuity store> is required")?;
    if all_sessions && !sessions.is_empty() {
        return Err("--all-sessions and --session are mutually exclusive".into());
    }
    if !all_sessions && sessions.is_empty() {
        return Err("select sessions with --session <uuid> (repeatable) or --all-sessions".into());
    }
    let mode = keep_newest.map_or(SelectionMode::ContentDedup, SelectionMode::KeepNewest);

    let local = Arc::new(LocalContinuityStore::open(&db)?);
    let targets: Vec<(Option<String>, meerkat_core::types::SessionId)> = if all_sessions {
        local
            .list_session_bindings()
            .await?
            .into_iter()
            .map(|(identity, session_id)| (Some(identity.to_string()), session_id))
            .collect()
    } else {
        let mut targets = Vec::new();
        for raw in &sessions {
            let session_id = meerkat_core::types::SessionId::parse(raw)
                .map_err(|e| format!("--session must be a session UUID: {e}"))?;
            targets.push((None, session_id));
        }
        targets
    };
    if targets.is_empty() {
        return Err("the continuity store holds no session bindings".into());
    }

    let continuity: Arc<dyn ContinuityStore> = local;
    let mut reports = Vec::with_capacity(targets.len());
    let mut refused = 0usize;
    for (identity_hint, session_id) in targets {
        eprintln!("mobkit-repair: session {session_id} ...");
        match repair_session(&continuity, &session_id, mode, apply).await {
            Ok(report) => reports.push(report),
            Err(error) => {
                refused += 1;
                eprintln!("mobkit-repair: session {session_id} REFUSED: {error}");
                reports.push(SessionReport {
                    session_id: session_id.to_string(),
                    identity: identity_hint,
                    outcome: "refused".to_string(),
                    detail: Some(error.to_string()),
                    rows_before: None,
                    rows_after: None,
                    bytes_before: None,
                    bytes_after: None,
                    system_rows_before: None,
                    system_rows_after: None,
                });
            }
        }
    }

    let report = RepairReport {
        mode: match mode {
            SelectionMode::ContentDedup => "content_dedup".to_string(),
            SelectionMode::KeepNewest(n) => format!("keep_newest_{n}"),
        },
        applied: apply,
        sessions: reports,
        refused,
        bytes_semantics: BYTES_SEMANTICS,
    };
    let rendered = serde_json::to_string_pretty(&report)?;
    println!("{rendered}");
    if let Some(path) = report_path {
        std::fs::write(&path, &rendered)?;
        eprintln!("mobkit-repair: report written to {path}");
    }
    if apply
        && report
            .sessions
            .iter()
            .any(|session| session.outcome == "applied")
    {
        // Step 5 of the procedure is LOAD-BEARING and easy to skip (called
        // out by two production operators): the runtime scratch store still
        // projects the PRE-repair transcript, and a boot that reads it will
        // serve the bloated head as if the repair never happened. Say it
        // loudly at the exact moment the operator is about to restart.
        eprintln!();
        eprintln!(
            "mobkit-repair: APPLY COMPLETE - ONE STEP REMAINS BEFORE RESTARTING THE GATEWAY:"
        );
        eprintln!(
            "mobkit-repair:   remove the runtime scratch store (the file set next to this \
             continuity store; `rkat storage doctor` names it)."
        );
        eprintln!(
            "mobkit-repair:   It still projects the PRE-repair transcript; the next boot must \
             mint runtime authority from the healed durable rows (the reset-reseed lane). \
             Skipping this step makes the repair appear to have not happened."
        );
        eprintln!(
            "mobkit-repair:   Note: the store FILE does not shrink - audited rewrites retain \
             prior strands; the healed HEAD is what got smaller (see bytes_semantics in the \
             report)."
        );
    }
    Ok(i32::from(refused != 0))
}

/// Repair one session through the typed rewrite door. Every failure is a
/// typed refusal returned to the fleet loop; the durable row is preserved
/// by the door's own guards on every refused path.
async fn repair_session(
    continuity: &Arc<dyn ContinuityStore>,
    session_id: &meerkat_core::types::SessionId,
    mode: SelectionMode,
    apply: bool,
) -> Result<SessionReport, Box<dyn std::error::Error>> {
    let adapter = Arc::new(ContinuitySessionStoreAdapter::new(Arc::clone(continuity)));

    // Write authority = the durable continuity record, exactly the facts a
    // registered resume would carry (the parked-repair contract).
    let (record, fencing_token, fence_current) = continuity
        .resolve_record_by_session(session_id)
        .await?
        .ok_or("no continuity record binds this session")?;
    adapter
        .register_session(
            session_id,
            SessionRuntimeState {
                identity: record.identity.clone(),
                generation: record.generation,
                fencing_token,
                checkpoint_version: fence_current,
            },
        )
        .await?;

    // Load the durable session (slim: content only), then HYDRATE the
    // out-of-line rewrite graph from the store's own rewrite rows onto it.
    // Slim materializations drop the compact graph by design - a rewrite
    // committed on one composes at generation 1 and the store refuses it
    // against the retained chain. With the store-proved graph installed the
    // rewrite composes at the correct next generation.
    let mut session = meerkat::SessionStore::load(adapter.as_ref(), session_id)
        .await?
        .ok_or("no durable row for this session")?;
    let channel = continuity
        .as_incremental_sessions()
        .ok_or("the continuity store provides no incremental channel")?;
    let rewrite_records = channel.load_rewrites(session_id).await?;
    if let Some(validated) =
        meerkat_core::ValidatedTranscriptHistory::from_rewrite_records_with_proved(
            rewrite_records,
            None,
        )?
    {
        session.install_validated_audited_transcript_history_preserving_live(validated)?;
    }

    let messages = session.messages();
    let serialized: Vec<Vec<u8>> = messages
        .iter()
        .map(serde_json::to_vec)
        .collect::<Result<_, _>>()?;
    let values: Vec<serde_json::Value> = serialized
        .iter()
        .map(|bytes| serde_json::from_slice(bytes))
        .collect::<Result<_, _>>()?;
    let is_system = |value: &serde_json::Value| -> bool {
        value
            .get("role")
            .and_then(|r| r.as_str())
            .map(|r| r == "system")
            .unwrap_or(false)
    };
    let system_positions: Vec<usize> = values
        .iter()
        .enumerate()
        .filter(|(_, value)| is_system(value))
        .map(|(index, _)| index)
        .collect();

    let duplicate_indices: Vec<usize> = match mode {
        SelectionMode::ContentDedup => {
            // Group System rows by (content, identity), ignoring only the
            // envelope timestamp; keep the FIRST occurrence per group.
            let mut seen_groups: std::collections::HashMap<String, usize> =
                std::collections::HashMap::new();
            let mut duplicates = Vec::new();
            for &index in &system_positions {
                let value = &values[index];
                let key = serde_json::to_string(&serde_json::json!({
                    "content": value.get("content"),
                    "identity": value.get("identity"),
                }))?;
                match seen_groups.entry(key) {
                    std::collections::hash_map::Entry::Vacant(slot) => {
                        slot.insert(index);
                    }
                    std::collections::hash_map::Entry::Occupied(_) => duplicates.push(index),
                }
            }
            duplicates
        }
        SelectionMode::KeepNewest(n) => {
            let cut = system_positions.len().saturating_sub(n);
            system_positions[..cut].to_vec()
        }
    };

    let rows_before = messages.len();
    let bytes_before: usize = serialized.iter().map(Vec::len).sum();
    let system_before = system_positions.len();
    if duplicate_indices.is_empty() {
        eprintln!(
            "mobkit-repair: session {session_id}: {system_before} System row(s), \
             nothing to drop"
        );
        return Ok(SessionReport {
            session_id: session_id.to_string(),
            identity: Some(record.identity.to_string()),
            outcome: "nothing_to_do".to_string(),
            detail: None,
            rows_before: Some(rows_before),
            rows_after: Some(rows_before),
            bytes_before: Some(bytes_before),
            bytes_after: Some(bytes_before),
            system_rows_before: Some(system_before),
            system_rows_after: Some(system_before),
        });
    }

    let cleaned: Vec<meerkat_core::Message> = messages
        .iter()
        .enumerate()
        .filter(|(index, _)| !duplicate_indices.contains(index))
        .map(|(_, message)| message.clone())
        .collect();
    let rows_after = cleaned.len();
    let bytes_after: usize = cleaned
        .iter()
        .map(|m| serde_json::to_vec(m).map(|v| v.len()).unwrap_or(0))
        .sum();
    let system_after = system_before - duplicate_indices.len();
    eprintln!(
        "mobkit-repair: session {session_id}: plan {rows_before} -> {rows_after} rows, \
         ~{bytes_before} -> ~{bytes_after} bytes, System {system_before} -> {system_after}"
    );

    if !apply {
        return Ok(SessionReport {
            session_id: session_id.to_string(),
            identity: Some(record.identity.to_string()),
            outcome: "dry_run".to_string(),
            detail: None,
            rows_before: Some(rows_before),
            rows_after: Some(rows_after),
            bytes_before: Some(bytes_before),
            bytes_after: Some(bytes_after),
            system_rows_before: Some(system_before),
            system_rows_after: Some(system_after),
        });
    }

    let parent_revision = session.transcript_revision()?;
    let selection_end = messages.len();
    let mut rewritten = session.clone();
    let commit = rewritten.commit_transcript_rewrite(
        meerkat_core::TranscriptRewriteSelection::MessageRange {
            start: 0,
            end: selection_end,
        },
        cleaned,
        meerkat_core::TranscriptRewriteReason::new(
            "operator prune of superseded System rows (mobkit-repair)",
        ),
        Some("mobkit-repair/system-rows".to_string()),
        Some(parent_revision),
    )?;
    meerkat::SessionStore::save_transcript_rewrite(adapter.as_ref(), &rewritten, &commit).await?;
    meerkat::SessionStore::save_authoritative_projection(adapter.as_ref(), &rewritten).await?;
    eprintln!(
        "mobkit-repair: session {session_id}: APPLIED at rewrite generation {}",
        commit.rewrite_generation
    );
    Ok(SessionReport {
        session_id: session_id.to_string(),
        identity: Some(record.identity.to_string()),
        outcome: "applied".to_string(),
        detail: None,
        rows_before: Some(rows_before),
        rows_after: Some(rows_after),
        bytes_before: Some(bytes_before),
        bytes_after: Some(bytes_after),
        system_rows_before: Some(system_before),
        system_rows_after: Some(system_after),
    })
}
