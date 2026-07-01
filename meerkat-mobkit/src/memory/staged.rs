//! Staged mutation batches and the deterministic commit validator.
//!
//! §8.5 crash semantics: LLM output (and the markdown import path) never
//! edits live records — it stages a `StagedMutationBatch`, a pure validator
//! checks it, and `commit` applies it atomically with one audit entry per
//! op. A producer that dies mid-run leaves a stage token that is
//! garbage-collected, never applied.
//!
//! The validator is deterministic code only: schema/caps, scope legality,
//! the §10.2 trust-tier transition lattice (including the transitive
//! provenance ceiling), supersede-chain acyclicity, and
//! tombstone-recreation rejection. Semantic judgment (is this merge right?)
//! is the steward's; it never runs here.

use std::collections::{HashMap, HashSet};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::records::{
    MemoryAuthor, MemoryId, MemoryScope, NewMemoryRecord, RecordStatus, TrustTier, content_hash,
    validate_record_fields,
};
use crate::identity_first::agent_memory::{AgentMemoryError, AgentMemoryProvider};

/// Default window inside which re-creating tombstoned content is rejected
/// (§8.4: a revocation-driven reset must not re-learn what was just
/// revoked).
pub const DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS: u64 = 7 * 24 * 60 * 60 * 1000;

/// Hard cap on ops per batch — a schema guard, not a retention policy.
pub const MAX_BATCH_OPS: usize = 1024;

/// One mutation in a staged batch. `Create`/`Supersede` accept an explicit
/// `id` so imports can preserve identifiers; `derived_from` records
/// consolidation lineage so the transitive ceiling can see merges.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum StagedOp {
    Create {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<MemoryId>,
        scope: MemoryScope,
        record: NewMemoryRecord,
        trust: TrustTier,
        #[serde(default)]
        derived_from: Vec<MemoryId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rationale: Option<String>,
        /// Import paths preserve original timestamps; `None` means now.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        created_at_ms: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        updated_at_ms: Option<u64>,
    },
    Supersede {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<MemoryId>,
        prior: MemoryId,
        record: NewMemoryRecord,
        trust: TrustTier,
        #[serde(default)]
        derived_from: Vec<MemoryId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rationale: Option<String>,
    },
    Tombstone {
        id: MemoryId,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rationale: Option<String>,
    },
    Retier {
        id: MemoryId,
        trust: TrustTier,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rationale: Option<String>,
    },
    SetRank {
        id: MemoryId,
        rank: Option<u32>,
    },
}

impl StagedOp {
    pub fn kind_str(&self) -> &'static str {
        match self {
            Self::Create { .. } => "create",
            Self::Supersede { .. } => "supersede",
            Self::Tombstone { .. } => "tombstone",
            Self::Retier { .. } => "retier",
            Self::SetRank { .. } => "set_rank",
        }
    }
}

/// A staged batch. Carries the realm (the store is per-realm; id-only ops
/// like `Tombstone` have no scope to infer it from) and one author for the
/// whole batch — a batch is one principal's proposal, and the lattice rules
/// key off authorship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedMutationBatch {
    pub realm: String,
    pub author: MemoryAuthor,
    pub ops: Vec<StagedOp>,
}

/// Opaque handle to a staged-but-uncommitted batch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StageToken {
    pub realm: String,
    pub token: String,
}

/// Result of an atomic commit: the affected record id per applied op, in op
/// order (`None` for ops that touch no single record id — currently none).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitReceipt {
    pub token: String,
    pub applied_ops: usize,
    pub memory_ids: Vec<MemoryId>,
}

/// The staging capability (§7.3): what consolidation requires, split from
/// the storage provider so read-only or simple backends need not implement
/// it. Atomicity is the implementor's contract — `commit` applies the whole
/// batch or none of it, with one audit entry per op.
#[async_trait]
pub trait StagedMemoryStore: AgentMemoryProvider {
    /// Validate the batch against current store state and persist it,
    /// unapplied, under a token. Stale tokens are garbage-collected.
    async fn stage(&self, batch: StagedMutationBatch) -> Result<StageToken, AgentMemoryError>;

    /// Re-validate and apply the staged batch in a single transaction,
    /// writing one audit entry per op and burning the token.
    async fn commit(&self, token: StageToken) -> Result<CommitReceipt, AgentMemoryError>;
}

/// What the validator needs to know about an existing record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedRecordView {
    pub scope: MemoryScope,
    pub trust: TrustTier,
    pub status: RecordStatus,
    pub supersedes: Option<MemoryId>,
    pub derived_from: Vec<MemoryId>,
    pub content_hash: String,
    pub has_verification: bool,
}

/// Read-only store view backing the validator, so validation stays pure
/// code that a SQLite transaction or a test fixture can implement.
pub trait StagedBatchView {
    fn record(&self, id: &str) -> Option<StagedRecordView>;
    /// Most recent tombstone time for content with this hash in this scope.
    fn tombstoned_at_ms(&self, scope: &MemoryScope, content_hash: &str) -> Option<u64>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagedBatchError {
    EmptyBatch,
    TooManyOps {
        max: usize,
        got: usize,
    },
    InvalidRecord {
        op_index: usize,
        reason: String,
    },
    RealmMismatch {
        op_index: usize,
        expected: String,
        got: String,
    },
    ScopeNotWritable {
        op_index: usize,
        reason: String,
    },
    TierNotStagedAssignable {
        op_index: usize,
        tier: TrustTier,
    },
    TierAboveAuthorCeiling {
        op_index: usize,
        tier: TrustTier,
        ceiling: TrustTier,
    },
    TransitiveTaintCeiling {
        op_index: usize,
        tier: TrustTier,
    },
    UnverifiedRetier {
        op_index: usize,
    },
    UnknownRecord {
        op_index: usize,
        id: MemoryId,
    },
    RecordExists {
        op_index: usize,
        id: MemoryId,
    },
    NotActive {
        op_index: usize,
        id: MemoryId,
    },
    AlreadyTombstoned {
        op_index: usize,
        id: MemoryId,
    },
    SupersedeCycle {
        op_index: usize,
        id: MemoryId,
    },
    TombstoneRecreation {
        op_index: usize,
        content_hash: String,
    },
}

impl std::fmt::Display for StagedBatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyBatch => write!(f, "staged batch must contain at least one op"),
            Self::TooManyOps { max, got } => {
                write!(f, "staged batch has {got} ops; the cap is {max}")
            }
            Self::InvalidRecord { op_index, reason } => {
                write!(f, "op {op_index}: invalid record: {reason}")
            }
            Self::RealmMismatch {
                op_index,
                expected,
                got,
            } => write!(
                f,
                "op {op_index}: scope realm '{got}' does not match batch realm '{expected}'"
            ),
            Self::ScopeNotWritable { op_index, reason } => {
                write!(f, "op {op_index}: scope not writable: {reason}")
            }
            Self::TierNotStagedAssignable { op_index, tier } => write!(
                f,
                "op {op_index}: trust tier '{}' is never assignable via a staged batch",
                tier.as_str()
            ),
            Self::TierAboveAuthorCeiling {
                op_index,
                tier,
                ceiling,
            } => write!(
                f,
                "op {op_index}: trust tier '{}' exceeds the author's ceiling '{}'",
                tier.as_str(),
                ceiling.as_str()
            ),
            Self::TransitiveTaintCeiling { op_index, tier } => write!(
                f,
                "op {op_index}: trust tier '{}' rejected: provenance chain reaches \
                 untrusted/quarantined content (capped at agent_observed)",
                tier.as_str()
            ),
            Self::UnverifiedRetier { op_index } => write!(
                f,
                "op {op_index}: retier to agent_verified requires a steward author and a \
                 verification claim on the record"
            ),
            Self::UnknownRecord { op_index, id } => {
                write!(f, "op {op_index}: record '{id}' does not exist")
            }
            Self::RecordExists { op_index, id } => {
                write!(f, "op {op_index}: record '{id}' already exists")
            }
            Self::NotActive { op_index, id } => {
                write!(f, "op {op_index}: record '{id}' is not active")
            }
            Self::AlreadyTombstoned { op_index, id } => {
                write!(f, "op {op_index}: record '{id}' is already tombstoned")
            }
            Self::SupersedeCycle { op_index, id } => {
                write!(
                    f,
                    "op {op_index}: supersede chain through '{id}' would form a cycle"
                )
            }
            Self::TombstoneRecreation {
                op_index,
                content_hash,
            } => write!(
                f,
                "op {op_index}: content (hash {content_hash}) was tombstoned within the \
                 recreation window and cannot be re-created"
            ),
        }
    }
}

impl std::error::Error for StagedBatchError {}

/// Deterministic batch validation. Pure: reads only `view` and its inputs.
///
/// Ops are checked in order against an overlay of the batch's own earlier
/// effects, so intra-batch sequences (create-then-supersede) validate the
/// same way they will apply.
pub fn validate_batch(
    batch: &StagedMutationBatch,
    view: &dyn StagedBatchView,
    tombstone_recreate_window_ms: u64,
    now_ms: u64,
) -> Result<(), StagedBatchError> {
    if batch.ops.is_empty() {
        return Err(StagedBatchError::EmptyBatch);
    }
    if batch.ops.len() > MAX_BATCH_OPS {
        return Err(StagedBatchError::TooManyOps {
            max: MAX_BATCH_OPS,
            got: batch.ops.len(),
        });
    }

    // Overlay of batch-local effects, keyed by record id. Only explicit-id
    // creates/supersedes are addressable by later ops.
    let mut overlay: HashMap<MemoryId, StagedRecordView> = HashMap::new();
    // Content tombstoned earlier in this same batch, per scope.
    let mut batch_tombstoned: HashSet<(MemoryScope, String)> = HashSet::new();

    let lookup =
        |overlay: &HashMap<MemoryId, StagedRecordView>, id: &str| -> Option<StagedRecordView> {
            overlay.get(id).cloned().or_else(|| view.record(id))
        };

    for (op_index, op) in batch.ops.iter().enumerate() {
        match op {
            StagedOp::Create {
                id,
                scope,
                record,
                trust,
                derived_from,
                ..
            } => {
                check_record_payload(op_index, record)?;
                check_scope(op_index, batch, scope)?;
                check_tier_assignment(op_index, batch, *trust, false)?;
                if let Some(id) = id
                    && lookup(&overlay, id).is_some()
                {
                    return Err(StagedBatchError::RecordExists {
                        op_index,
                        id: id.clone(),
                    });
                }
                for source in derived_from {
                    if lookup(&overlay, source).is_none() {
                        return Err(StagedBatchError::UnknownRecord {
                            op_index,
                            id: source.clone(),
                        });
                    }
                }
                let tainted = chain_reaches_taint(&overlay, view, derived_from.iter());
                if tainted && *trust > TrustTier::llm_write_ceiling() {
                    return Err(StagedBatchError::TransitiveTaintCeiling {
                        op_index,
                        tier: *trust,
                    });
                }
                let hash = content_hash(&record.title, &record.body);
                check_tombstone_recreation(
                    op_index,
                    &batch.author,
                    view,
                    &batch_tombstoned,
                    scope,
                    &hash,
                    tombstone_recreate_window_ms,
                    now_ms,
                )?;
                if let Some(id) = id {
                    overlay.insert(
                        id.clone(),
                        StagedRecordView {
                            scope: scope.clone(),
                            trust: *trust,
                            status: RecordStatus::Active,
                            supersedes: None,
                            derived_from: derived_from.clone(),
                            content_hash: hash,
                            has_verification: record.verification.is_some(),
                        },
                    );
                }
            }
            StagedOp::Supersede {
                id,
                prior,
                record,
                trust,
                derived_from,
                ..
            } => {
                check_record_payload(op_index, record)?;
                check_tier_assignment(op_index, batch, *trust, false)?;
                let Some(prior_view) = lookup(&overlay, prior) else {
                    return Err(StagedBatchError::UnknownRecord {
                        op_index,
                        id: prior.clone(),
                    });
                };
                if prior_view.status != RecordStatus::Active {
                    return Err(StagedBatchError::NotActive {
                        op_index,
                        id: prior.clone(),
                    });
                }
                // The new record lives in the prior's scope (§8.2: supersede
                // stays within a single record's lineage in its own scope).
                check_scope(op_index, batch, &prior_view.scope)?;
                if let Some(id) = id
                    && lookup(&overlay, id).is_some()
                {
                    return Err(StagedBatchError::RecordExists {
                        op_index,
                        id: id.clone(),
                    });
                }
                for source in derived_from {
                    if lookup(&overlay, source).is_none() {
                        return Err(StagedBatchError::UnknownRecord {
                            op_index,
                            id: source.clone(),
                        });
                    }
                }
                // Acyclicity: explicit-id supersedes can express cycles
                // (imports of chains); walk the prior's chain with the new
                // edge in place.
                if let Some(new_id) = id {
                    let mut visited = HashSet::new();
                    visited.insert(new_id.clone());
                    let mut cursor = Some(prior.clone());
                    while let Some(current) = cursor {
                        if !visited.insert(current.clone()) {
                            return Err(StagedBatchError::SupersedeCycle {
                                op_index,
                                id: current,
                            });
                        }
                        cursor = lookup(&overlay, &current).and_then(|r| r.supersedes);
                    }
                }
                let tainted = chain_reaches_taint(
                    &overlay,
                    view,
                    std::iter::once(prior).chain(derived_from.iter()),
                );
                if tainted && *trust > TrustTier::llm_write_ceiling() {
                    return Err(StagedBatchError::TransitiveTaintCeiling {
                        op_index,
                        tier: *trust,
                    });
                }
                let hash = content_hash(&record.title, &record.body);
                check_tombstone_recreation(
                    op_index,
                    &batch.author,
                    view,
                    &batch_tombstoned,
                    &prior_view.scope,
                    &hash,
                    tombstone_recreate_window_ms,
                    now_ms,
                )?;
                overlay.insert(
                    prior.clone(),
                    StagedRecordView {
                        status: RecordStatus::Superseded {
                            by: id.clone().unwrap_or_else(|| "<pending>".to_string()),
                        },
                        ..prior_view.clone()
                    },
                );
                if let Some(id) = id {
                    overlay.insert(
                        id.clone(),
                        StagedRecordView {
                            scope: prior_view.scope.clone(),
                            trust: *trust,
                            status: RecordStatus::Active,
                            supersedes: Some(prior.clone()),
                            derived_from: derived_from.clone(),
                            content_hash: hash,
                            has_verification: record.verification.is_some(),
                        },
                    );
                }
            }
            StagedOp::Tombstone { id, .. } => {
                let Some(existing) = lookup(&overlay, id) else {
                    return Err(StagedBatchError::UnknownRecord {
                        op_index,
                        id: id.clone(),
                    });
                };
                if existing.status == RecordStatus::Tombstoned {
                    return Err(StagedBatchError::AlreadyTombstoned {
                        op_index,
                        id: id.clone(),
                    });
                }
                check_scope(op_index, batch, &existing.scope)?;
                batch_tombstoned.insert((existing.scope.clone(), existing.content_hash.clone()));
                overlay.insert(
                    id.clone(),
                    StagedRecordView {
                        status: RecordStatus::Tombstoned,
                        ..existing
                    },
                );
            }
            StagedOp::Retier { id, trust, .. } => {
                check_tier_assignment(op_index, batch, *trust, true)?;
                let Some(existing) = lookup(&overlay, id) else {
                    return Err(StagedBatchError::UnknownRecord {
                        op_index,
                        id: id.clone(),
                    });
                };
                if existing.status == RecordStatus::Tombstoned {
                    return Err(StagedBatchError::AlreadyTombstoned {
                        op_index,
                        id: id.clone(),
                    });
                }
                check_scope(op_index, batch, &existing.scope)?;
                if *trust == TrustTier::AgentVerified {
                    // §10.2: agent_verified is granted only by a steward
                    // staged op against a record carrying a verification
                    // claim. (Evidence-ref resolvability and the dream's
                    // endorsement are the P3 steward's half.)
                    let steward = matches!(batch.author, MemoryAuthor::Steward { .. });
                    if !steward || !existing.has_verification {
                        return Err(StagedBatchError::UnverifiedRetier { op_index });
                    }
                }
                if *trust > TrustTier::llm_write_ceiling() {
                    let tainted = chain_reaches_taint(&overlay, view, std::iter::once(id));
                    if tainted {
                        return Err(StagedBatchError::TransitiveTaintCeiling {
                            op_index,
                            tier: *trust,
                        });
                    }
                }
                overlay.insert(
                    id.clone(),
                    StagedRecordView {
                        trust: *trust,
                        ..existing
                    },
                );
            }
            StagedOp::SetRank { id, .. } => {
                let Some(existing) = lookup(&overlay, id) else {
                    return Err(StagedBatchError::UnknownRecord {
                        op_index,
                        id: id.clone(),
                    });
                };
                if existing.status == RecordStatus::Tombstoned {
                    return Err(StagedBatchError::AlreadyTombstoned {
                        op_index,
                        id: id.clone(),
                    });
                }
                check_scope(op_index, batch, &existing.scope)?;
            }
        }
    }
    Ok(())
}

fn check_record_payload(op_index: usize, record: &NewMemoryRecord) -> Result<(), StagedBatchError> {
    validate_record_fields(&record.title, &record.description, &record.body)
        .map_err(|reason| StagedBatchError::InvalidRecord { op_index, reason })
}

/// Scope legality (§7.2 write authority): scopes must live in the batch
/// realm; agents write only their own identity scope (mob/operator writes
/// are proposals, realm scope is application-side); steward/distiller and
/// non-LLM principals may stage into any in-realm scope.
fn check_scope(
    op_index: usize,
    batch: &StagedMutationBatch,
    scope: &MemoryScope,
) -> Result<(), StagedBatchError> {
    if scope.realm() != batch.realm {
        return Err(StagedBatchError::RealmMismatch {
            op_index,
            expected: batch.realm.clone(),
            got: scope.realm().to_string(),
        });
    }
    if let MemoryAuthor::Agent { identity } = &batch.author {
        match scope {
            MemoryScope::Identity {
                identity: scope_identity,
                ..
            } if scope_identity == identity => {}
            other => {
                return Err(StagedBatchError::ScopeNotWritable {
                    op_index,
                    reason: format!(
                        "agent '{identity}' may only write its own identity scope, not \
                         {} scope (mob/operator writes go through proposals)",
                        other.kind_str()
                    ),
                });
            }
        }
    }
    Ok(())
}

fn check_tier_assignment(
    op_index: usize,
    batch: &StagedMutationBatch,
    tier: TrustTier,
    is_retier: bool,
) -> Result<(), StagedBatchError> {
    if !tier.assignable_via_staged_batch() {
        return Err(StagedBatchError::TierNotStagedAssignable { op_index, tier });
    }
    // §10.2: LLM-authored writes enter at agent_observed or below — steward
    // included. The single exception is a steward *retier* to
    // agent_verified, whose claim requirement is checked at the call site.
    if batch.author.is_llm() && tier > TrustTier::llm_write_ceiling() {
        let steward_retier = is_retier && matches!(batch.author, MemoryAuthor::Steward { .. });
        if !steward_retier {
            return Err(StagedBatchError::TierAboveAuthorCeiling {
                op_index,
                tier,
                ceiling: TrustTier::llm_write_ceiling(),
            });
        }
    }
    Ok(())
}

/// Walks supersede + derivation edges from the given starting ids; true if
/// any reachable record is `Untrusted` or `Quarantined` (§10.2 transitive
/// provenance ceiling).
fn chain_reaches_taint<'a>(
    overlay: &HashMap<MemoryId, StagedRecordView>,
    view: &dyn StagedBatchView,
    start: impl Iterator<Item = &'a MemoryId>,
) -> bool {
    let mut stack: Vec<MemoryId> = start.cloned().collect();
    let mut visited: HashSet<MemoryId> = HashSet::new();
    while let Some(id) = stack.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let Some(record) = overlay.get(&id).cloned().or_else(|| view.record(&id)) else {
            continue;
        };
        if record.trust == TrustTier::Untrusted
            || matches!(record.status, RecordStatus::Quarantined { .. })
        {
            return true;
        }
        if let Some(prior) = record.supersedes {
            stack.push(prior);
        }
        stack.extend(record.derived_from.iter().cloned());
    }
    false
}

/// §8.4 Distiller guard: LLM authors must not re-learn content that was
/// just revoked. Deliberate non-LLM re-adds (operator/SDK forget-then-
/// remember) are a human decision and pass.
fn check_tombstone_recreation(
    op_index: usize,
    author: &MemoryAuthor,
    view: &dyn StagedBatchView,
    batch_tombstoned: &HashSet<(MemoryScope, String)>,
    scope: &MemoryScope,
    hash: &str,
    window_ms: u64,
    now_ms: u64,
) -> Result<(), StagedBatchError> {
    if !author.is_llm() {
        return Ok(());
    }
    if batch_tombstoned.contains(&(scope.clone(), hash.to_string())) {
        return Err(StagedBatchError::TombstoneRecreation {
            op_index,
            content_hash: hash.to_string(),
        });
    }
    if let Some(at_ms) = view.tombstoned_at_ms(scope, hash)
        && now_ms.saturating_sub(at_ms) <= window_ms
    {
        return Err(StagedBatchError::TombstoneRecreation {
            op_index,
            content_hash: hash.to_string(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::records::MemoryKind;

    struct MapView {
        records: HashMap<MemoryId, StagedRecordView>,
        tombstoned: HashMap<(MemoryScope, String), u64>,
    }

    impl MapView {
        fn empty() -> Self {
            Self {
                records: HashMap::new(),
                tombstoned: HashMap::new(),
            }
        }
    }

    impl StagedBatchView for MapView {
        fn record(&self, id: &str) -> Option<StagedRecordView> {
            self.records.get(id).cloned()
        }

        fn tombstoned_at_ms(&self, scope: &MemoryScope, hash: &str) -> Option<u64> {
            self.tombstoned
                .get(&(scope.clone(), hash.to_string()))
                .copied()
        }
    }

    fn identity_scope() -> MemoryScope {
        MemoryScope::Identity {
            realm: "family".to_string(),
            identity: "identity:luka".to_string(),
        }
    }

    fn payload(title: &str, body: &str) -> NewMemoryRecord {
        NewMemoryRecord {
            kind: MemoryKind::Fact,
            title: title.to_string(),
            description: String::new(),
            body: body.to_string(),
            tags: Vec::new(),
            evidence: Vec::new(),
            verification: None,
        }
    }

    fn record_view(scope: MemoryScope, trust: TrustTier, status: RecordStatus) -> StagedRecordView {
        StagedRecordView {
            scope,
            trust,
            status,
            supersedes: None,
            derived_from: Vec::new(),
            content_hash: content_hash("t", "b"),
            has_verification: false,
        }
    }

    fn agent_batch(ops: Vec<StagedOp>) -> StagedMutationBatch {
        StagedMutationBatch {
            realm: "family".to_string(),
            author: MemoryAuthor::Agent {
                identity: "identity:luka".to_string(),
            },
            ops,
        }
    }

    fn steward_batch(ops: Vec<StagedOp>) -> StagedMutationBatch {
        StagedMutationBatch {
            realm: "family".to_string(),
            author: MemoryAuthor::Steward {
                run_id: "dream-1".to_string(),
            },
            ops,
        }
    }

    fn create_op(scope: MemoryScope, trust: TrustTier) -> StagedOp {
        StagedOp::Create {
            id: None,
            scope,
            record: payload("Fact title", "Fact body"),
            trust,
            derived_from: Vec::new(),
            rationale: None,
            created_at_ms: None,
            updated_at_ms: None,
        }
    }

    #[test]
    fn empty_batch_rejected() {
        let err = validate_batch(
            &agent_batch(Vec::new()),
            &MapView::empty(),
            DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS,
            1_000,
        )
        .expect_err("empty batch");
        assert_eq!(err, StagedBatchError::EmptyBatch);
    }

    #[test]
    fn operator_and_application_tiers_rejected_for_every_author() {
        for author in [
            MemoryAuthor::Operator,
            MemoryAuthor::Application,
            MemoryAuthor::Steward {
                run_id: "d".to_string(),
            },
        ] {
            for tier in [TrustTier::Operator, TrustTier::Application] {
                let batch = StagedMutationBatch {
                    realm: "family".to_string(),
                    author: author.clone(),
                    ops: vec![create_op(identity_scope(), tier)],
                };
                let err = validate_batch(
                    &batch,
                    &MapView::empty(),
                    DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS,
                    1_000,
                )
                .expect_err("high tier must be rejected");
                assert_eq!(
                    err,
                    StagedBatchError::TierNotStagedAssignable { op_index: 0, tier }
                );
            }
        }
    }

    #[test]
    fn agent_create_above_observed_rejected() {
        let err = validate_batch(
            &agent_batch(vec![create_op(identity_scope(), TrustTier::AgentVerified)]),
            &MapView::empty(),
            DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS,
            1_000,
        )
        .expect_err("agent above ceiling");
        assert_eq!(
            err,
            StagedBatchError::TierAboveAuthorCeiling {
                op_index: 0,
                tier: TrustTier::AgentVerified,
                ceiling: TrustTier::AgentObserved,
            }
        );
    }

    #[test]
    fn agent_cannot_write_mob_scope() {
        let mob_scope = MemoryScope::Mob {
            realm: "family".to_string(),
            mob: "mob:home".to_string(),
        };
        let err = validate_batch(
            &agent_batch(vec![create_op(mob_scope, TrustTier::AgentObserved)]),
            &MapView::empty(),
            DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS,
            1_000,
        )
        .expect_err("mob scope is proposal-only for agents");
        assert!(matches!(
            err,
            StagedBatchError::ScopeNotWritable { op_index: 0, .. }
        ));
    }

    #[test]
    fn realm_mismatch_rejected() {
        let other_realm = MemoryScope::Identity {
            realm: "work".to_string(),
            identity: "identity:luka".to_string(),
        };
        let err = validate_batch(
            &agent_batch(vec![create_op(other_realm, TrustTier::AgentObserved)]),
            &MapView::empty(),
            DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS,
            1_000,
        )
        .expect_err("cross-realm scope");
        assert!(matches!(
            err,
            StagedBatchError::RealmMismatch { op_index: 0, .. }
        ));
    }

    #[test]
    fn oversized_payload_rejected() {
        let mut op = create_op(identity_scope(), TrustTier::AgentObserved);
        if let StagedOp::Create { record, .. } = &mut op {
            record.body = "b".repeat(MAX_RECORD_BODY_BYTES_PLUS_ONE);
        }
        const MAX_RECORD_BODY_BYTES_PLUS_ONE: usize =
            crate::memory::records::MAX_RECORD_BODY_BYTES + 1;
        let err = validate_batch(
            &agent_batch(vec![op]),
            &MapView::empty(),
            DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS,
            1_000,
        )
        .expect_err("oversized body");
        assert!(matches!(
            err,
            StagedBatchError::InvalidRecord { op_index: 0, .. }
        ));
    }

    #[test]
    fn supersede_of_missing_or_inactive_prior_rejected() {
        let mut view = MapView::empty();
        view.records.insert(
            "mem-superseded".to_string(),
            record_view(
                identity_scope(),
                TrustTier::AgentObserved,
                RecordStatus::Superseded {
                    by: "mem-new".to_string(),
                },
            ),
        );
        let missing = agent_batch(vec![StagedOp::Supersede {
            id: None,
            prior: "mem-missing".to_string(),
            record: payload("t", "b"),
            trust: TrustTier::AgentObserved,
            derived_from: Vec::new(),
            rationale: None,
        }]);
        assert!(matches!(
            validate_batch(&missing, &view, DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS, 1_000),
            Err(StagedBatchError::UnknownRecord { op_index: 0, .. })
        ));
        let inactive = agent_batch(vec![StagedOp::Supersede {
            id: None,
            prior: "mem-superseded".to_string(),
            record: payload("t", "b"),
            trust: TrustTier::AgentObserved,
            derived_from: Vec::new(),
            rationale: None,
        }]);
        assert!(matches!(
            validate_batch(
                &inactive,
                &view,
                DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS,
                1_000
            ),
            Err(StagedBatchError::NotActive { op_index: 0, .. })
        ));
    }

    #[test]
    fn explicit_id_supersede_cycle_rejected() {
        // Import-shaped batch: A supersedes B, then B supersedes A.
        let batch = steward_batch(vec![
            StagedOp::Create {
                id: Some("mem-b".to_string()),
                scope: identity_scope(),
                record: payload("b", "body b"),
                trust: TrustTier::AgentObserved,
                derived_from: Vec::new(),
                rationale: None,
                created_at_ms: None,
                updated_at_ms: None,
            },
            StagedOp::Supersede {
                id: Some("mem-a".to_string()),
                prior: "mem-b".to_string(),
                record: payload("a", "body a"),
                trust: TrustTier::AgentObserved,
                derived_from: Vec::new(),
                rationale: None,
            },
            StagedOp::Supersede {
                id: Some("mem-b2".to_string()),
                prior: "mem-a".to_string(),
                record: payload("b2", "body b2"),
                trust: TrustTier::AgentObserved,
                derived_from: Vec::new(),
                rationale: None,
            },
        ]);
        // The straight chain is fine.
        validate_batch(
            &batch,
            &MapView::empty(),
            DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS,
            1_000,
        )
        .expect("acyclic chain validates");

        // Reusing an id already in the chain must fail (either as an
        // existing-id conflict or as a detected cycle) — the store can never
        // apply a cyclic supersede graph.
        let cyclic = steward_batch(vec![
            StagedOp::Create {
                id: Some("mem-b".to_string()),
                scope: identity_scope(),
                record: payload("b", "body b"),
                trust: TrustTier::AgentObserved,
                derived_from: Vec::new(),
                rationale: None,
                created_at_ms: None,
                updated_at_ms: None,
            },
            StagedOp::Supersede {
                id: Some("mem-a".to_string()),
                prior: "mem-b".to_string(),
                record: payload("a", "body a"),
                trust: TrustTier::AgentObserved,
                derived_from: Vec::new(),
                rationale: None,
            },
            StagedOp::Supersede {
                id: Some("mem-b".to_string()),
                prior: "mem-a".to_string(),
                record: payload("b3", "body b3"),
                trust: TrustTier::AgentObserved,
                derived_from: Vec::new(),
                rationale: None,
            },
        ]);
        let err = validate_batch(
            &cyclic,
            &MapView::empty(),
            DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS,
            1_000,
        )
        .expect_err("cycle must be rejected");
        assert!(matches!(
            err,
            StagedBatchError::RecordExists { op_index: 2, .. }
                | StagedBatchError::SupersedeCycle { op_index: 2, .. }
        ));
    }

    #[test]
    fn cycle_through_preexisting_chain_rejected() {
        // Store already has A(active) supersedes B. A batch that recreates
        // "mem-b" as a supersede of A would close the loop A -> B -> A.
        let mut view = MapView::empty();
        view.records.insert(
            "mem-a".to_string(),
            StagedRecordView {
                supersedes: Some("mem-b".to_string()),
                ..record_view(
                    identity_scope(),
                    TrustTier::AgentObserved,
                    RecordStatus::Active,
                )
            },
        );
        let batch = steward_batch(vec![StagedOp::Supersede {
            id: Some("mem-b".to_string()),
            prior: "mem-a".to_string(),
            record: payload("b", "body b"),
            trust: TrustTier::AgentObserved,
            derived_from: Vec::new(),
            rationale: None,
        }]);
        let err = validate_batch(&batch, &view, DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS, 1_000)
            .expect_err("preexisting-chain cycle must be rejected");
        assert!(matches!(
            err,
            StagedBatchError::SupersedeCycle { op_index: 0, .. }
        ));
    }

    #[test]
    fn transitive_taint_laundering_rejected() {
        // Quarantined record Q; active P supersedes U (untrusted). Both
        // laundering routes must be capped:
        // 1. Steward merges Q into a "fresh" record and retiers it up.
        let mut view = MapView::empty();
        view.records.insert(
            "mem-q".to_string(),
            record_view(
                identity_scope(),
                TrustTier::AgentObserved,
                RecordStatus::Quarantined {
                    reason: "tainted session".to_string(),
                },
            ),
        );
        let launder_by_merge = steward_batch(vec![
            StagedOp::Create {
                id: Some("mem-fresh".to_string()),
                scope: identity_scope(),
                record: {
                    let mut p = payload("Consolidated", "Merged content");
                    p.verification = Some(crate::memory::records::VerificationClaim {
                        checked: "verified against evidence".to_string(),
                        evidence: Vec::new(),
                    });
                    p
                },
                trust: TrustTier::AgentObserved,
                derived_from: vec!["mem-q".to_string()],
                rationale: Some("consolidation".to_string()),
                created_at_ms: None,
                updated_at_ms: None,
            },
            StagedOp::Retier {
                id: "mem-fresh".to_string(),
                trust: TrustTier::AgentVerified,
                rationale: Some("launder attempt".to_string()),
            },
        ]);
        let err = validate_batch(
            &launder_by_merge,
            &view,
            DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS,
            1_000,
        )
        .expect_err("laundering by consolidation must be rejected");
        assert_eq!(
            err,
            StagedBatchError::TransitiveTaintCeiling {
                op_index: 1,
                tier: TrustTier::AgentVerified,
            }
        );

        // 2. Non-LLM author supersedes a record whose chain reaches
        //    untrusted provenance, asking for agent_verified.
        let mut view = MapView::empty();
        view.records.insert(
            "mem-u".to_string(),
            record_view(
                identity_scope(),
                TrustTier::Untrusted,
                RecordStatus::Superseded {
                    by: "mem-p".to_string(),
                },
            ),
        );
        view.records.insert(
            "mem-p".to_string(),
            StagedRecordView {
                supersedes: Some("mem-u".to_string()),
                ..record_view(
                    identity_scope(),
                    TrustTier::AgentObserved,
                    RecordStatus::Active,
                )
            },
        );
        let launder_by_supersede = StagedMutationBatch {
            realm: "family".to_string(),
            author: MemoryAuthor::Application,
            ops: vec![StagedOp::Supersede {
                id: None,
                prior: "mem-p".to_string(),
                record: payload("Fresh", "Fresh body"),
                trust: TrustTier::AgentVerified,
                derived_from: Vec::new(),
                rationale: None,
            }],
        };
        let err = validate_batch(
            &launder_by_supersede,
            &view,
            DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS,
            1_000,
        )
        .expect_err("tainted supersede chain must cap the tier");
        assert_eq!(
            err,
            StagedBatchError::TransitiveTaintCeiling {
                op_index: 0,
                tier: TrustTier::AgentVerified,
            }
        );
    }

    #[test]
    fn retier_to_verified_requires_steward_and_claim() {
        let mut view = MapView::empty();
        view.records.insert(
            "mem-1".to_string(),
            record_view(
                identity_scope(),
                TrustTier::AgentObserved,
                RecordStatus::Active,
            ),
        );
        // No verification claim: even the steward is rejected.
        let steward = steward_batch(vec![StagedOp::Retier {
            id: "mem-1".to_string(),
            trust: TrustTier::AgentVerified,
            rationale: None,
        }]);
        assert_eq!(
            validate_batch(&steward, &view, DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS, 1_000),
            Err(StagedBatchError::UnverifiedRetier { op_index: 0 })
        );
        // With a claim, the steward passes and an agent author still fails.
        view.records
            .get_mut("mem-1")
            .expect("present")
            .has_verification = true;
        validate_batch(&steward, &view, DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS, 1_000)
            .expect("steward retier with claim");
        let agent = agent_batch(vec![StagedOp::Retier {
            id: "mem-1".to_string(),
            trust: TrustTier::AgentVerified,
            rationale: None,
        }]);
        assert_eq!(
            validate_batch(&agent, &view, DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS, 1_000),
            Err(StagedBatchError::TierAboveAuthorCeiling {
                op_index: 0,
                tier: TrustTier::AgentVerified,
                ceiling: TrustTier::AgentObserved,
            }),
            "non-steward LLM authors hit the blanket ceiling before the retier rule"
        );
    }

    #[test]
    fn tombstone_recreation_rejected_inside_window_only() {
        let scope = identity_scope();
        let hash = content_hash("Fact title", "Fact body");
        let mut view = MapView::empty();
        view.tombstoned.insert((scope.clone(), hash), 1_000);

        let batch = steward_batch(vec![create_op(scope.clone(), TrustTier::AgentObserved)]);
        // Inside the window: reject.
        assert!(matches!(
            validate_batch(&batch, &view, 10_000, 5_000),
            Err(StagedBatchError::TombstoneRecreation { op_index: 0, .. })
        ));
        // Outside the window: allowed.
        validate_batch(&batch, &view, 10_000, 20_000).expect("window expired");
    }

    #[test]
    fn tombstone_then_recreate_within_one_batch_rejected() {
        let mut view = MapView::empty();
        view.records.insert(
            "mem-1".to_string(),
            StagedRecordView {
                content_hash: content_hash("Fact title", "Fact body"),
                ..record_view(
                    identity_scope(),
                    TrustTier::AgentObserved,
                    RecordStatus::Active,
                )
            },
        );
        let batch = steward_batch(vec![
            StagedOp::Tombstone {
                id: "mem-1".to_string(),
                rationale: None,
            },
            create_op(identity_scope(), TrustTier::AgentObserved),
        ]);
        assert!(matches!(
            validate_batch(&batch, &view, DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS, 1_000),
            Err(StagedBatchError::TombstoneRecreation { op_index: 1, .. })
        ));
    }
}
