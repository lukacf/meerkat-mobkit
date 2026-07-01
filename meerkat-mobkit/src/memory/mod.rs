//! MobKit agent-memory record layer (docs/design/agent-memory-architecture.md).
//!
//! Deterministic structure only — record model and scopes (`records`),
//! staged mutations with the commit validator (`staged`), and the bundled
//! per-realm SQLite store (`sqlite_store`). This module sits inside the
//! §12 bright-line ratchet (`scripts/check-memory-bright-line`): it must
//! never grow retrieval-index machinery; that is hub material.

pub mod records;
pub mod sqlite_store;
pub mod staged;

pub use records::{
    CalibrationRef, EvidenceRef, ManifestTier, MemoryAuthor, MemoryId, MemoryKind,
    MemoryProvenance, MemoryRecord, MemoryScope, NewMemoryRecord, ProposalId, RecordMeta,
    RecordStatus, TrustTier, UsageEvent, UsageStats, VerificationClaim, content_hash,
};
pub use sqlite_store::SqliteAgentMemoryStore;
pub use staged::{
    CommitReceipt, StageToken, StagedBatchError, StagedBatchView, StagedMemoryStore,
    StagedMutationBatch, StagedOp, StagedRecordView, validate_batch,
};
