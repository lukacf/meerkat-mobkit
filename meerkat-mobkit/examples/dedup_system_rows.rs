//! Operator surgery helper (task #58, HomeCore parent-1 context cliff):
//! drop byte-identical duplicate System rows from a live mob-hosted
//! session's durable transcript, keeping the first copy, through the
//! typed rewrite door - one full-range rewrite commit composed onto the
//! retained graph, never a hand edit of store rows.
//!
//! Procedure (single-writer discipline):
//!   1. STOP the gateway.
//!   2. Back up continuity.db (byte copy).
//!   3. Dry run:  cargo run --example dedup_system_rows -- \
//!        --db /path/to/continuity.db --session <uuid>
//!   4. Apply:    ... --apply
//!   5. Remove the runtime scratch store (runtime.db) so the next boot
//!      mints runtime authority from the healed durable row (the
//!      fleet-proven reset-reseed lane).
//!   6. Start the gateway.

use std::sync::Arc;

use meerkat_mobkit::identity_first::{
    ContinuitySessionStoreAdapter, ContinuityStore, LocalContinuityStore, SessionRuntimeState,
};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut db: Option<String> = None;
    let mut session: Option<String> = None;
    let mut apply = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--db" => db = args.next(),
            "--session" => session = args.next(),
            "--apply" => apply = true,
            other => return Err(format!("unknown argument: {other}").into()),
        }
    }
    let db = db.ok_or("--db <continuity.db> is required")?;
    let session_id =
        meerkat_core::types::SessionId::parse(&session.ok_or("--session <uuid> is required")?)
            .map_err(|e| format!("--session must be a session UUID: {e}"))?;

    let continuity: Arc<dyn ContinuityStore> = Arc::new(LocalContinuityStore::open(&db)?);
    let adapter = Arc::new(ContinuitySessionStoreAdapter::new(Arc::clone(&continuity)));

    // Write authority = the durable continuity record, exactly the facts a
    // registered resume would carry (the parked-repair contract).
    let (record, fencing_token, fence_current) = continuity
        .resolve_record_by_session(&session_id)
        .await?
        .ok_or("no continuity record binds this session")?;
    adapter
        .register_session(
            &session_id,
            SessionRuntimeState {
                identity: record.identity.clone(),
                generation: record.generation,
                fencing_token,
                checkpoint_version: fence_current,
            },
        )
        .await?;

    let session = meerkat::SessionStore::load(adapter.as_ref(), &session_id)
        .await?
        .ok_or("no durable row for this session")?;
    let messages = session.messages();
    println!(
        "session {session_id}: {} messages, identity {}",
        messages.len(),
        record.identity
    );

    // Byte-identical System duplicates of the FIRST System row.
    let serialized: Vec<Vec<u8>> = messages
        .iter()
        .map(serde_json::to_vec)
        .collect::<Result<_, _>>()?;
    let is_system = |bytes: &[u8]| -> bool {
        serde_json::from_slice::<serde_json::Value>(bytes)
            .ok()
            .and_then(|v| {
                v.get("role")
                    .and_then(|r| r.as_str())
                    .map(|r| r == "system")
            })
            .unwrap_or(false)
    };
    let first_system = serialized.iter().position(|bytes| is_system(bytes));
    let Some(first_system) = first_system else {
        println!("no System rows at all; nothing to do");
        return Ok(());
    };
    let reference = &serialized[first_system];
    let duplicate_indices: Vec<usize> = serialized
        .iter()
        .enumerate()
        .skip(first_system + 1)
        .filter(|(_, bytes)| *bytes == reference)
        .map(|(index, _)| index)
        .collect();
    println!(
        "first System row at index {first_system} ({} bytes); {} byte-identical duplicates",
        reference.len(),
        duplicate_indices.len()
    );
    if duplicate_indices.is_empty() {
        println!("nothing to drop");
        return Ok(());
    }

    let cleaned: Vec<meerkat_core::Message> = messages
        .iter()
        .enumerate()
        .filter(|(index, _)| !duplicate_indices.contains(index))
        .map(|(_, message)| message.clone())
        .collect();
    let before_bytes: usize = serialized.iter().map(Vec::len).sum();
    let after_bytes: usize = cleaned
        .iter()
        .map(|m| serde_json::to_vec(m).map(|v| v.len()).unwrap_or(0))
        .sum();
    println!(
        "rewrite plan: {} -> {} messages, ~{} -> ~{} bytes",
        messages.len(),
        cleaned.len(),
        before_bytes,
        after_bytes
    );

    if !apply {
        println!("DRY RUN ONLY - re-run with --apply to commit");
        return Ok(());
    }

    let parent_revision = session.transcript_revision()?;
    let mut rewritten = session.clone();
    let commit = rewritten.commit_transcript_rewrite(
        meerkat_core::TranscriptRewriteSelection::MessageRange {
            start: 0,
            end: messages.len(),
        },
        cleaned,
        meerkat_core::TranscriptRewriteReason::new(
            "operator dedup of byte-identical authored System copies",
        ),
        Some("homecore-operator/dedup_system_rows".to_string()),
        Some(parent_revision),
    )?;
    meerkat::SessionStore::save_transcript_rewrite(adapter.as_ref(), &rewritten, &commit).await?;
    meerkat::SessionStore::save_authoritative_projection(adapter.as_ref(), &rewritten).await?;
    println!(
        "APPLIED: rewrite generation {} committed, durable head now {} messages at revision {}",
        commit.rewrite_generation,
        rewritten.messages().len(),
        rewritten.transcript_revision()?
    );
    println!("now remove the runtime scratch store and boot; the mint reseeds from durable");
    Ok(())
}
