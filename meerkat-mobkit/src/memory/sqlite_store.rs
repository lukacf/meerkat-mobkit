//! Bundled per-realm SQLite agent-memory store (§7.3).
//!
//! One database per realm at `<root>/<pct-encoded-realm>.sqlite3` — the same
//! directory and encoding scheme the markdown store uses (deliberately NOT
//! `<persistent_state>/memory/`, which belongs to meerkat's session semantic
//! memory). WAL journaling, busy-timeout, plain B-tree lookups only: the
//! bright-line ratchet (§12) forbids retrieval-index machinery here, and
//! recall quality is the LLM Selector's job, not the store's.
//!
//! Every write path — including `remember`/`forget` and the markdown import
//! — flows through the staged-batch validator and a single-transaction
//! apply with one audit row per op (§8.5 crash semantics).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use rusqlite::{Connection, OptionalExtension, params};

use crate::identity_first::AgentIdentity;
use crate::identity_first::agent_memory::{
    AgentMemoryError, AgentMemoryForgetResult, AgentMemoryProvider, AgentMemoryRecallRequest,
    AgentMemoryRecord, AuthoredWriteReceipt, NewAgentMemory, compact_whitespace,
    decode_path_segment, encode_path_segment, new_memory_id, normalize_tags, read_markdown_records,
    select_recall_records,
};
use crate::memory::taint::LlmWriteGate;

use super::records::{
    InjectionLogEntry, InjectionSurface, ManifestTier, MemoryAuthor, MemoryId, MemoryKind,
    MemoryProvenance, MemoryScope, NewMemoryRecord, ProposalId, RecordMeta, RecordStatus,
    TrustTier, UsageEvent, UsageStats, age_days, content_hash, validate_record_fields,
};
use super::staged::{
    CommitReceipt, DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS, StageToken, StagedBatchKind,
    StagedBatchView, StagedMemoryStore, StagedMutationBatch, StagedOp, StagedRecordView,
    validate_batch,
};

/// Per-scope retention floors (§7.3): exceeded floors WARN the steward via
/// tracing; deterministic code never evicts.
pub const DEFAULT_SCOPE_FLOOR_RECORDS: usize = 4_000;
pub const DEFAULT_SCOPE_FLOOR_BYTES: usize = 32 * 1024 * 1024;

/// Staged-but-uncommitted batches older than this are garbage-collected on
/// realm open — a dead producer leaves a token that is never applied.
const STAGE_GC_MAX_AGE_MS: u64 = 24 * 60 * 60 * 1000;

const SQLITE_BUSY_TIMEOUT_MS: u64 = 5_000;

const SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS records (
    memory_id       TEXT PRIMARY KEY,
    scope_kind      TEXT NOT NULL,
    scope_key       TEXT NOT NULL,
    kind            TEXT NOT NULL,
    title           TEXT NOT NULL,
    description     TEXT NOT NULL DEFAULT '',
    body            TEXT NOT NULL,
    tags            TEXT NOT NULL DEFAULT '[]',
    provenance      TEXT NOT NULL,
    trust           TEXT NOT NULL,
    status_kind     TEXT NOT NULL,
    status_detail   TEXT,
    supersedes      TEXT,
    derived_from    TEXT NOT NULL DEFAULT '[]',
    working_set_rank INTEGER,
    rank_set_at_ms  INTEGER,
    content_hash    TEXT NOT NULL,
    created_at_ms   INTEGER NOT NULL,
    updated_at_ms   INTEGER NOT NULL,
    usage_stats     TEXT NOT NULL DEFAULT '{}',
    tombstoned_at_ms INTEGER,
    -- §10.2 durable taint marker: 1 when the record landed quarantined or
    -- descends from a record that did. Survives the tombstone that a
    -- quarantine release applies to the origin (which erases the
    -- `quarantined` status), so the transitive ceiling holds forever.
    ever_quarantined INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS records_scope_idx
    ON records(scope_kind, scope_key, status_kind);
CREATE INDEX IF NOT EXISTS records_scope_hash_idx
    ON records(scope_kind, scope_key, content_hash);

CREATE TABLE IF NOT EXISTS proposals (
    proposal_id   TEXT PRIMARY KEY,
    scope_kind    TEXT NOT NULL,
    scope_key     TEXT NOT NULL,
    record        TEXT NOT NULL,
    author        TEXT NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending',
    created_at_ms INTEGER NOT NULL,
    -- §10.1: quarantine decision captured AT PROPOSE TIME (the taint
    -- tracker is in-memory and session-sticky; re-deriving at dream time
    -- both under- and over-quarantines). NULL = clean at propose time.
    taint         TEXT
);

CREATE TABLE IF NOT EXISTS audit (
    audit_id      INTEGER PRIMARY KEY AUTOINCREMENT,
    stage_token   TEXT NOT NULL,
    op_index      INTEGER NOT NULL,
    op_kind       TEXT NOT NULL,
    memory_id     TEXT,
    detail        TEXT NOT NULL,
    applied_at_ms INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS stage (
    token         TEXT PRIMARY KEY,
    batch         TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL
);

-- Injection ledger (§9.2): plain telemetry appends, deliberately outside
-- the staged-batch path — rows here are observations about delivery, not
-- record mutations. session_key is NULL for build-time assembly, where the
-- session does not exist yet.
CREATE TABLE IF NOT EXISTS injections (
    injection_id  INTEGER PRIMARY KEY AUTOINCREMENT,
    record_id     TEXT NOT NULL,
    identity      TEXT NOT NULL,
    session_key   TEXT,
    surface       TEXT NOT NULL,
    at_ms         INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS injections_record_idx
    ON injections(record_id, at_ms);

-- Exit-interview queue (§8.5): identities recorded by the retire/delete
-- hooks; the next dream harvests each pending row and marks it done.
CREATE TABLE IF NOT EXISTS pending_harvests (
    identity      TEXT NOT NULL,
    session_key   TEXT,
    cause         TEXT NOT NULL,
    retired_at_ms INTEGER NOT NULL,
    status        TEXT NOT NULL DEFAULT 'pending',
    PRIMARY KEY (identity, retired_at_ms)
);

-- Quarantine-promotions awaiting operator approval through the gating
-- flow (§10.2): gating pending_id → staged batch token. Only a gating
-- approval commits the token; deny/timeout discards it.
CREATE TABLE IF NOT EXISTS pending_promotions (
    pending_id     TEXT PRIMARY KEY,
    stage_token    TEXT NOT NULL,
    record_id      TEXT NOT NULL,
    scope_kind     TEXT NOT NULL,
    scope_key      TEXT NOT NULL,
    rationale      TEXT,
    status         TEXT NOT NULL DEFAULT 'pending',
    created_at_ms  INTEGER NOT NULL,
    resolved_at_ms INTEGER
);
";

const RECORD_COLUMNS: &str = "memory_id, scope_kind, scope_key, kind, title, description, body, \
     tags, provenance, trust, status_kind, status_detail, supersedes, derived_from, \
     working_set_rank, rank_set_at_ms, content_hash, created_at_ms, updated_at_ms, \
     usage_stats, tombstoned_at_ms";

/// Bundled SQLite store. Cheap to clone; connections are cached per realm
/// and shared across clones.
#[derive(Clone)]
pub struct SqliteAgentMemoryStore {
    root: PathBuf,
    scope_floor_records: usize,
    scope_floor_bytes: usize,
    connections: Arc<Mutex<HashMap<String, Arc<Mutex<Connection>>>>>,
    /// §10.1 write-seam enforcement: consulted for every LLM-authored
    /// create/supersede across ALL write paths (direct and staged commits),
    /// so taint/posture quarantine holds for any caller — the Recorder
    /// tool, staged batches, and future stages alike. Shared across clones
    /// so wiring the gate once covers every handle.
    llm_write_gate: Arc<Mutex<Option<Arc<dyn LlmWriteGate>>>>,
    /// §10.2 P3 extension: evidence-ref resolvability for `agent_verified`
    /// retiers. Optional like the write gate — the wiring that enables the
    /// steward installs it; absent, the P2 claim-presence rule stands
    /// alone. Shared across clones.
    evidence_resolver: Arc<Mutex<Option<Arc<dyn EvidenceRefResolver>>>>,
    /// §9.3 timeline sink for quarantined-write events. Shared across
    /// clones; absent, the tracing warn is the only surface.
    event_sink: Arc<Mutex<Option<Arc<dyn crate::memory::events::MemoryEventSink>>>>,
}

/// §10.2 P3: whether an [`EvidenceRef`] resolves against the persistent
/// session store (session exists; a cited range lies within the persisted
/// transcript). The semantic endorsement half of an `agent_verified` retier
/// is the dream's judgment (recorded in the op rationale); this is the
/// mechanical half.
pub trait EvidenceRefResolver: Send + Sync {
    fn resolves(&self, evidence: &crate::memory::records::EvidenceRef) -> Result<(), String>;
}

impl SqliteAgentMemoryStore {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, AgentMemoryError> {
        let root = root.into();
        if root.as_os_str().is_empty() {
            return Err(AgentMemoryError::InvalidConfig(
                "agent memory root path must not be empty".to_string(),
            ));
        }
        fs::create_dir_all(&root).map_err(|err| AgentMemoryError::Io(err.to_string()))?;
        Ok(Self {
            root,
            scope_floor_records: DEFAULT_SCOPE_FLOOR_RECORDS,
            scope_floor_bytes: DEFAULT_SCOPE_FLOOR_BYTES,
            connections: Arc::new(Mutex::new(HashMap::new())),
            llm_write_gate: Arc::new(Mutex::new(None)),
            evidence_resolver: Arc::new(Mutex::new(None)),
            event_sink: Arc::new(Mutex::new(None)),
        })
    }

    /// Install the §10.1 LLM write gate. Wiring installs it at startup,
    /// before any member can dispatch a write.
    pub fn set_llm_write_gate(&self, gate: Arc<dyn LlmWriteGate>) {
        *self
            .llm_write_gate
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some(gate);
    }

    /// Install the §10.2 evidence-ref resolver. The steward wiring installs
    /// it at startup; from then on every staged retier to `agent_verified`
    /// must cite evidence that resolves against the session store.
    pub fn set_evidence_resolver(&self, resolver: Arc<dyn EvidenceRefResolver>) {
        *self
            .evidence_resolver
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some(resolver);
    }

    /// Wire the §9.3 timeline sink for quarantined-write events.
    pub fn set_event_sink(&self, sink: Arc<dyn crate::memory::events::MemoryEventSink>) {
        *self
            .event_sink
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some(sink);
    }

    fn gate(&self) -> Option<Arc<dyn LlmWriteGate>> {
        self.llm_write_gate
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn resolver(&self) -> Option<Arc<dyn EvidenceRefResolver>> {
        self.evidence_resolver
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    fn events(&self) -> Option<Arc<dyn crate::memory::events::MemoryEventSink>> {
        self.event_sink
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
    }

    #[cfg(test)]
    fn with_scope_floors(mut self, records: usize, bytes: usize) -> Self {
        self.scope_floor_records = records;
        self.scope_floor_bytes = bytes;
        self
    }

    /// Same directory + percent-encoding scheme as
    /// `MarkdownAgentMemoryStore::path_for`, one database per realm.
    pub fn path_for_realm(&self, realm: &str) -> PathBuf {
        self.root
            .join(format!("{}.sqlite3", encode_path_segment(realm)))
    }

    fn realm_connection(&self, realm: &str) -> Result<Arc<Mutex<Connection>>, AgentMemoryError> {
        let mut connections = self
            .connections
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        if let Some(existing) = connections.get(realm) {
            return Ok(existing.clone());
        }
        let mut conn = Connection::open(self.path_for_realm(realm)).map_err(sql_err)?;
        conn.busy_timeout(std::time::Duration::from_millis(SQLITE_BUSY_TIMEOUT_MS))
            .map_err(sql_err)?;
        // WAL survives crashes without blocking readers; query_row because
        // the pragma returns the new mode.
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))
            .map_err(sql_err)?;
        conn.execute_batch(SCHEMA_SQL).map_err(sql_err)?;
        // Column migrations for stores created before the columns joined
        // SCHEMA_SQL (CREATE TABLE IF NOT EXISTS never alters).
        if ensure_column(
            &conn,
            "records",
            "ever_quarantined",
            "INTEGER NOT NULL DEFAULT 0",
        )? {
            // Backfill the durable §10.2 marker: currently-quarantined rows
            // directly; tombstoned rows through their audit trail (the
            // tombstone apply nulls status_detail, so the audit row's
            // `"quarantined":"<reason>"` is the only remaining evidence
            // that a row once landed quarantined).
            conn.execute(
                "UPDATE records SET ever_quarantined = 1 WHERE status_kind = 'quarantined'",
                [],
            )
            .map_err(sql_err)?;
            conn.execute(
                "UPDATE records SET ever_quarantined = 1 WHERE status_kind = 'tombstoned' \
                 AND memory_id IN (SELECT memory_id FROM audit \
                 WHERE detail LIKE '%\"quarantined\":\"%')",
                [],
            )
            .map_err(sql_err)?;
        }
        if ensure_column(&conn, "proposals", "taint", "TEXT")? {
            // Conservative backfill (mirrors ever_quarantined above): the
            // propose-time taint fact for pre-migration proposals lived only
            // in the in-memory SessionTaintTracker and is unrecoverable, so
            // still-live proposals route through the operator-gated
            // promotion path instead of reading as clean. Terminal statuses
            // (accepted/rejected) are never re-verdicted and stay untouched.
            conn.execute(
                "UPDATE proposals SET taint = 'pre-migration proposal: propose-time \
                 taint fact unrecoverable' WHERE status IN ('pending', 'held')",
                [],
            )
            .map_err(sql_err)?;
        }
        let now = now_ms();
        // Stage GC spares tokens referenced by a still-pending gated
        // promotion (§10.2) — the operator's decision window outranks the
        // dead-producer sweep; deny/timeout resolution discards them.
        conn.execute(
            "DELETE FROM stage WHERE created_at_ms < ?1 AND token NOT IN \
             (SELECT stage_token FROM pending_promotions WHERE status = 'pending')",
            params![(now.saturating_sub(STAGE_GC_MAX_AGE_MS)) as i64],
        )
        .map_err(sql_err)?;
        self.import_markdown_realm(&mut conn, realm)?;
        let shared = Arc::new(Mutex::new(conn));
        connections.insert(realm.to_string(), shared.clone());
        Ok(shared)
    }

    /// One-shot migration (§7.3): un-imported markdown files for this realm
    /// are imported through the staged-commit path (ids and timestamps
    /// preserved; kind=fact, trust=agent_observed, identity scope, agent
    /// author with empty evidence) and renamed to `<file>.imported` —
    /// user-inspectable data is never deleted.
    ///
    /// §7.3 invites hand edits, so content problems must never make the
    /// realm store unopenable: an invalid record is skipped loudly (warn +
    /// count in the import audit row) and the rest of the file imports; a
    /// file that fails wholesale (bad identity stem, over the size cap,
    /// residual batch-validation failure) is warned about, set aside as
    /// `<file>.import-failed`, and the remaining files continue. Only real
    /// I/O errors propagate into the open.
    fn import_markdown_realm(
        &self,
        conn: &mut Connection,
        realm: &str,
    ) -> Result<(), AgentMemoryError> {
        let realm_dir = self.root.join(encode_path_segment(realm));
        if !realm_dir.is_dir() {
            return Ok(());
        }
        let entries =
            fs::read_dir(&realm_dir).map_err(|err| AgentMemoryError::Io(err.to_string()))?;
        let mut files: Vec<PathBuf> = entries
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
            .collect();
        files.sort();
        for path in files {
            match self.import_markdown_file(conn, realm, &path) {
                Ok(()) => {}
                Err(MarkdownImportError::Content(reason)) => {
                    tracing::warn!(
                        file = %path.display(),
                        reason,
                        "agent memory markdown import: file failed and was set aside as \
                         .import-failed (fix and rename back to .md to retry); the realm \
                         store stays open"
                    );
                    record_import_audit(conn, &path, 0, 1, std::slice::from_ref(&reason))?;
                    let mut failed_name = path.as_os_str().to_owned();
                    failed_name.push(".import-failed");
                    fs::rename(&path, PathBuf::from(failed_name))
                        .map_err(|err| AgentMemoryError::Io(err.to_string()))?;
                }
                Err(MarkdownImportError::Io(err)) => return Err(err),
            }
        }
        Ok(())
    }

    fn import_markdown_file(
        &self,
        conn: &mut Connection,
        realm: &str,
        path: &Path,
    ) -> Result<(), MarkdownImportError> {
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            return Ok(());
        };
        let identity_str = decode_path_segment(stem);
        let identity = AgentIdentity::parse(&identity_str).map_err(|err| {
            MarkdownImportError::Content(format!(
                "'{}' does not decode to an agent identity: {err}",
                path.display()
            ))
        })?;
        let records = read_markdown_records(path).map_err(|err| match err {
            AgentMemoryError::Io(_) => MarkdownImportError::Io(err),
            other => MarkdownImportError::Content(other.to_string()),
        })?;
        let scope = MemoryScope::Identity {
            realm: realm.to_string(),
            identity: identity.as_str().to_string(),
        };
        // Skip ids already present (idempotence if a rename previously
        // failed) and dedup ids within the file (hand-edits happen).
        let mut seen = std::collections::HashSet::new();
        let mut ops = Vec::new();
        let mut skip_reasons: Vec<String> = Vec::new();
        for record in records {
            if !seen.insert(record.memory_id.clone()) {
                continue;
            }
            let exists: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM records WHERE memory_id = ?1",
                    params![record.memory_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sql_err)
                .map_err(MarkdownImportError::Io)?;
            if exists.is_some() {
                continue;
            }
            // Pre-validate each record with the same deterministic checks
            // the staged validator applies, so one bad hand-edited record
            // skips loudly instead of failing the whole batch.
            let mut skip = |record_id: &str, reason: String| {
                tracing::warn!(
                    file = %path.display(),
                    memory_id = record_id,
                    reason,
                    "agent memory markdown import: record skipped"
                );
                skip_reasons.push(format!("{record_id}: {reason}"));
            };
            if let Err(reason) = validate_record_fields(&record.title, "", &record.body) {
                skip(&record.memory_id, reason);
                continue;
            }
            if let Some(class) = crate::memory::secrets::detect_record_secret(
                &record.title,
                "",
                &record.body,
                &record.tags,
            ) {
                skip(
                    &record.memory_id,
                    format!("matches the '{class}' secret pattern class (§10.4)"),
                );
                continue;
            }
            ops.push(StagedOp::Create {
                id: Some(record.memory_id),
                scope: scope.clone(),
                record: NewMemoryRecord {
                    kind: MemoryKind::Fact,
                    title: record.title,
                    description: String::new(),
                    body: record.body,
                    tags: record.tags,
                    evidence: Vec::new(),
                    verification: None,
                },
                trust: TrustTier::AgentObserved,
                derived_from: Vec::new(),
                rationale: Some("markdown import".to_string()),
                created_at_ms: Some(record.created_at_ms),
                updated_at_ms: Some(record.updated_at_ms),
            });
        }
        let imported = ops.len();
        if !ops.is_empty() {
            let batch = StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: realm.to_string(),
                author: MemoryAuthor::Agent {
                    identity: identity.as_str().to_string(),
                },
                ops,
            };
            let token = mint_token("import");
            // Gate deliberately absent: the import migrates records the
            // markdown store already accepted; it is not a new LLM write.
            apply_batch_tx(conn, &batch, None, None, &token, now_ms()).map_err(|err| {
                MarkdownImportError::Content(format!("batch validation failed: {err}"))
            })?;
        }
        if !skip_reasons.is_empty() {
            record_import_audit(conn, path, imported, skip_reasons.len(), &skip_reasons)
                .map_err(MarkdownImportError::Io)?;
        }
        let mut imported_name = path.as_os_str().to_owned();
        imported_name.push(".imported");
        fs::rename(path, PathBuf::from(imported_name))
            .map_err(|err| MarkdownImportError::Io(AgentMemoryError::Io(err.to_string())))?;
        Ok(())
    }

    fn with_realm_conn<T>(
        &self,
        realm: &str,
        f: impl FnOnce(&mut Connection) -> Result<T, AgentMemoryError>,
    ) -> Result<T, AgentMemoryError> {
        let conn = self.realm_connection(realm)?;
        let mut guard = conn.lock().unwrap_or_else(|err| err.into_inner());
        f(&mut guard)
    }

    fn recall_blocking(
        &self,
        request: AgentMemoryRecallRequest,
    ) -> Result<Vec<AgentMemoryRecord>, AgentMemoryError> {
        let scope = MemoryScope::Identity {
            realm: request.realm.clone(),
            identity: request.identity.as_str().to_string(),
        };
        let records =
            self.with_realm_conn(&request.realm, |conn| active_scope_records(conn, &scope))?;
        let projected = records.into_iter().map(project_record).collect();
        Ok(select_recall_records(projected, &request))
    }

    fn remember_blocking(
        &self,
        realm: &str,
        identity: &AgentIdentity,
        memory: NewAgentMemory,
    ) -> Result<AgentMemoryRecord, AgentMemoryError> {
        let title = compact_whitespace(&memory.title);
        let body = memory.body.trim().to_string();
        validate_record_fields(&title, "", &body).map_err(AgentMemoryError::InvalidRecord)?;
        let tags = normalize_tags(memory.tags)?;
        let scope = MemoryScope::Identity {
            realm: realm.to_string(),
            identity: identity.as_str().to_string(),
        };
        let hash = content_hash(&title, &body);
        let floor_records = self.scope_floor_records;
        let floor_bytes = self.scope_floor_bytes;
        let gate = self.gate();
        let events = self.events();
        self.with_realm_conn(realm, |conn| {
            // Deterministic write guard (§7.3): an exact content-hash
            // duplicate short-circuits to the existing id — no new row.
            let existing: Option<MemoryRecordRow> = conn
                .query_row(
                    &format!(
                        "SELECT {RECORD_COLUMNS} FROM records \
                         WHERE scope_kind = ?1 AND scope_key = ?2 AND content_hash = ?3 \
                           AND status_kind = 'active' \
                         ORDER BY created_at_ms ASC LIMIT 1"
                    ),
                    params![scope.kind_str(), scope.key(), hash],
                    row_to_record_row,
                )
                .optional()
                .map_err(sql_err)?;
            if let Some(row) = existing {
                return Ok(project_record(row.into_record(scope.realm())?));
            }
            let batch = StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: realm.to_string(),
                // RPC/SDK writes are application-principal writes (§7.2);
                // the P1 Recorder threads real agent authorship.
                author: MemoryAuthor::Application,
                ops: vec![StagedOp::Create {
                    id: None,
                    scope: scope.clone(),
                    record: NewMemoryRecord {
                        kind: MemoryKind::Fact,
                        title,
                        description: String::new(),
                        body,
                        tags: tags.clone(),
                        evidence: Vec::new(),
                        verification: None,
                    },
                    trust: TrustTier::AgentObserved,
                    derived_from: Vec::new(),
                    rationale: None,
                    created_at_ms: None,
                    updated_at_ms: None,
                }],
            };
            let receipt = apply_batch_tx(
                conn,
                &batch,
                gate.as_deref(),
                events.as_deref(),
                &mint_token("direct"),
                now_ms(),
            )?;
            warn_if_scope_floors_exceeded(conn, &scope, floor_records, floor_bytes)?;
            let memory_id = receipt.memory_ids.first().cloned().ok_or_else(|| {
                AgentMemoryError::Io("remember commit returned no record id".to_string())
            })?;
            let record = load_record(conn, scope.realm(), &memory_id)?.ok_or_else(|| {
                AgentMemoryError::Io("remembered record vanished mid-commit".to_string())
            })?;
            Ok(project_record(record))
        })
    }

    fn forget_blocking(
        &self,
        realm: &str,
        identity: &AgentIdentity,
        memory_id: &str,
    ) -> Result<AgentMemoryForgetResult, AgentMemoryError> {
        let memory_id = memory_id.trim().to_string();
        if memory_id.is_empty() {
            return Err(AgentMemoryError::InvalidRecord(
                "memory_id must not be empty".to_string(),
            ));
        }
        let scope = MemoryScope::Identity {
            realm: realm.to_string(),
            identity: identity.as_str().to_string(),
        };
        self.forget_in_scope_blocking(&scope, &memory_id, MemoryAuthor::Application)
    }

    /// Shared tombstone path for the wire `forget` (Application principal)
    /// and the Recorder's `forget_authored` (Agent principal).
    fn forget_in_scope_blocking(
        &self,
        scope: &MemoryScope,
        memory_id: &str,
        author: MemoryAuthor,
    ) -> Result<AgentMemoryForgetResult, AgentMemoryError> {
        let memory_id = memory_id.to_string();
        let gate = self.gate();
        let events = self.events();
        self.with_realm_conn(scope.realm(), |conn| {
            let record = load_record(conn, scope.realm(), &memory_id)?;
            let deletable = record.is_some_and(|record| {
                record.scope == *scope && record.status != RecordStatus::Tombstoned
            });
            if !deletable {
                return Ok(AgentMemoryForgetResult {
                    memory_id,
                    deleted: false,
                });
            }
            let batch = StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: scope.realm().to_string(),
                author,
                ops: vec![StagedOp::Tombstone {
                    id: memory_id.clone(),
                    rationale: None,
                }],
            };
            apply_batch_tx(
                conn,
                &batch,
                gate.as_deref(),
                events.as_deref(),
                &mint_token("direct"),
                now_ms(),
            )?;
            Ok(AgentMemoryForgetResult {
                memory_id,
                deleted: true,
            })
        })
    }

    fn supersede_blocking(
        &self,
        scope: &MemoryScope,
        prior: &str,
        record: NewMemoryRecord,
    ) -> Result<MemoryId, AgentMemoryError> {
        self.supersede_with_author_blocking(scope, prior, record, MemoryAuthor::Application)
            .map(|receipt| receipt.memory_id)
    }

    fn supersede_with_author_blocking(
        &self,
        scope: &MemoryScope,
        prior: &str,
        record: NewMemoryRecord,
        author: MemoryAuthor,
    ) -> Result<AuthoredWriteReceipt, AgentMemoryError> {
        let title = compact_whitespace(&record.title);
        let body = record.body.trim().to_string();
        validate_record_fields(&title, &record.description, &body)
            .map_err(AgentMemoryError::InvalidRecord)?;
        let tags = normalize_tags(record.tags)?;
        let realm = scope.realm().to_string();
        let expected_scope = scope.clone();
        let gate = self.gate();
        let events = self.events();
        self.with_realm_conn(&realm, |conn| {
            let existing = load_record(conn, &realm, prior)?.ok_or_else(|| {
                AgentMemoryError::InvalidRecord(format!("record '{prior}' does not exist"))
            })?;
            if existing.scope != expected_scope {
                return Err(AgentMemoryError::InvalidRecord(format!(
                    "record '{prior}' does not belong to the requested scope"
                )));
            }
            let batch = StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: realm.clone(),
                author,
                ops: vec![StagedOp::Supersede {
                    id: None,
                    prior: prior.to_string(),
                    record: NewMemoryRecord {
                        title,
                        body,
                        tags,
                        ..record
                    },
                    trust: TrustTier::AgentObserved,
                    derived_from: Vec::new(),
                    rationale: None,
                }],
            };
            let receipt = apply_batch_tx(
                conn,
                &batch,
                gate.as_deref(),
                events.as_deref(),
                &mint_token("direct"),
                now_ms(),
            )?;
            let memory_id = receipt.memory_ids.first().cloned().ok_or_else(|| {
                AgentMemoryError::Io("supersede commit returned no record id".to_string())
            })?;
            let record = load_record(conn, &realm, &memory_id)?.ok_or_else(|| {
                AgentMemoryError::Io("superseding record vanished mid-commit".to_string())
            })?;
            Ok(AuthoredWriteReceipt {
                memory_id,
                status: record.status,
            })
        })
    }

    /// §8.2 Recorder create: agent-authored, gate-enforced, dedup-guarded.
    fn remember_authored_blocking(
        &self,
        scope: &MemoryScope,
        record: NewMemoryRecord,
        author: MemoryAuthor,
    ) -> Result<AuthoredWriteReceipt, AgentMemoryError> {
        let title = compact_whitespace(&record.title);
        let body = record.body.trim().to_string();
        validate_record_fields(&title, &record.description, &body)
            .map_err(AgentMemoryError::InvalidRecord)?;
        let tags = normalize_tags(record.tags)?;
        let hash = content_hash(&title, &body);
        let realm = scope.realm().to_string();
        let scope = scope.clone();
        let floor_records = self.scope_floor_records;
        let floor_bytes = self.scope_floor_bytes;
        let gate = self.gate();
        let events = self.events();
        self.with_realm_conn(&realm, |conn| {
            // Deterministic write guard (§7.3): an exact content-hash
            // duplicate short-circuits to the existing active record.
            let existing: Option<MemoryRecordRow> = conn
                .query_row(
                    &format!(
                        "SELECT {RECORD_COLUMNS} FROM records \
                         WHERE scope_kind = ?1 AND scope_key = ?2 AND content_hash = ?3 \
                           AND status_kind = 'active' \
                         ORDER BY created_at_ms ASC LIMIT 1"
                    ),
                    params![scope.kind_str(), scope.key(), hash],
                    row_to_record_row,
                )
                .optional()
                .map_err(sql_err)?;
            if let Some(row) = existing {
                let record = row.into_record(scope.realm())?;
                return Ok(AuthoredWriteReceipt {
                    memory_id: record.id,
                    status: record.status,
                });
            }
            let batch = StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: realm.clone(),
                author,
                ops: vec![StagedOp::Create {
                    id: None,
                    scope: scope.clone(),
                    record: NewMemoryRecord {
                        title,
                        body,
                        tags,
                        ..record
                    },
                    // §10.2: LLM writes enter at the ceiling; the staged
                    // validator rejects anything higher.
                    trust: TrustTier::AgentObserved,
                    derived_from: Vec::new(),
                    rationale: None,
                    created_at_ms: None,
                    updated_at_ms: None,
                }],
            };
            let receipt = apply_batch_tx(
                conn,
                &batch,
                gate.as_deref(),
                events.as_deref(),
                &mint_token("direct"),
                now_ms(),
            )?;
            warn_if_scope_floors_exceeded(conn, &scope, floor_records, floor_bytes)?;
            let memory_id = receipt.memory_ids.first().cloned().ok_or_else(|| {
                AgentMemoryError::Io("remember commit returned no record id".to_string())
            })?;
            let record = load_record(conn, scope.realm(), &memory_id)?.ok_or_else(|| {
                AgentMemoryError::Io("remembered record vanished mid-commit".to_string())
            })?;
            Ok(AuthoredWriteReceipt {
                memory_id,
                status: record.status,
            })
        })
    }

    fn manifest_blocking(
        &self,
        scopes: &[MemoryScope],
        tier: ManifestTier,
    ) -> Result<Vec<RecordMeta>, AgentMemoryError> {
        let now = now_ms();
        let mut out = Vec::new();
        for scope in scopes {
            let metas =
                self.with_realm_conn(scope.realm(), |conn| scope_manifest(conn, scope, tier, now))?;
            out.extend(metas);
        }
        Ok(out)
    }

    fn mark_usage_blocking(
        &self,
        ids: &[MemoryId],
        event: UsageEvent,
    ) -> Result<(), AgentMemoryError> {
        let now = now_ms();
        for realm in self.known_realms()? {
            self.with_realm_conn(&realm, |conn| {
                for id in ids {
                    let usage_json: Option<String> = conn
                        .query_row(
                            "SELECT usage_stats FROM records WHERE memory_id = ?1",
                            params![id],
                            |row| row.get(0),
                        )
                        .optional()
                        .map_err(sql_err)?;
                    let Some(usage_json) = usage_json else {
                        continue;
                    };
                    let mut usage: UsageStats =
                        serde_json::from_str(&usage_json).unwrap_or_default();
                    match event {
                        UsageEvent::Injected => {
                            usage.injected_count += 1;
                            usage.last_injected_at_ms = Some(now);
                        }
                        // Counted apart from ambient injection (§9.2): a
                        // pull on purpose is a much stronger usefulness
                        // signal than a push that may have been ignored.
                        UsageEvent::ExplicitRecall => {
                            usage.explicit_recall_count += 1;
                            usage.last_recalled_at_ms = Some(now);
                        }
                        UsageEvent::JudgedUseful => {
                            usage.judged_useful_count += 1;
                            usage.last_useful_at_ms = Some(now);
                        }
                    }
                    conn.execute(
                        "UPDATE records SET usage_stats = ?1 WHERE memory_id = ?2",
                        params![json_string(&usage)?, id],
                    )
                    .map_err(sql_err)?;
                }
                Ok(())
            })?;
        }
        Ok(())
    }

    fn log_injections_blocking(
        &self,
        realm: &str,
        entries: &[InjectionLogEntry],
    ) -> Result<(), AgentMemoryError> {
        if entries.is_empty() {
            return Ok(());
        }
        self.with_realm_conn(realm, |conn| {
            let mut stmt = conn
                .prepare(
                    "INSERT INTO injections (record_id, identity, session_key, surface, at_ms) \
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(sql_err)?;
            for entry in entries {
                stmt.execute(params![
                    entry.record_id,
                    entry.identity,
                    entry.session_key,
                    entry.surface.as_str(),
                    entry.at_ms as i64,
                ])
                .map_err(sql_err)?;
            }
            Ok(())
        })
    }

    fn injection_log_blocking(
        &self,
        realm: &str,
        limit: usize,
    ) -> Result<Vec<InjectionLogEntry>, AgentMemoryError> {
        self.with_realm_conn(realm, |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT record_id, identity, session_key, surface, at_ms FROM injections \
                     ORDER BY injection_id DESC LIMIT ?1",
                )
                .map_err(sql_err)?;
            let rows = stmt
                .query_map(params![limit as i64], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                })
                .map_err(sql_err)?;
            let mut entries = Vec::new();
            for row in rows {
                let (record_id, identity, session_key, surface, at_ms) = row.map_err(sql_err)?;
                let surface = InjectionSurface::parse(&surface).ok_or_else(|| {
                    AgentMemoryError::Parse(format!("unknown injection surface '{surface}'"))
                })?;
                entries.push(InjectionLogEntry {
                    record_id,
                    identity,
                    session_key,
                    surface,
                    at_ms: at_ms as u64,
                });
            }
            Ok(entries)
        })
    }

    /// Newest-first injection-ledger rows for a realm (§9.2). Read surface
    /// for the steward's usage audit and the console Memory panel.
    pub async fn injection_log(
        &self,
        realm: &str,
        limit: usize,
    ) -> Result<Vec<InjectionLogEntry>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        run_blocking(move || store.injection_log_blocking(&realm, limit)).await
    }

    fn propose_blocking(
        &self,
        scope: &MemoryScope,
        record: NewMemoryRecord,
        author: MemoryAuthor,
    ) -> Result<ProposalId, AgentMemoryError> {
        validate_record_fields(&record.title, &record.description, &record.body)
            .map_err(AgentMemoryError::InvalidRecord)?;
        // §10.4 secret hygiene: proposals bypass the staged validator (the
        // row is not a record yet), so the write-seam refusal is applied
        // here directly.
        if let Some(class) = crate::memory::secrets::detect_record_secret(
            &record.title,
            &record.description,
            &record.body,
            &record.tags,
        ) {
            return Err(AgentMemoryError::InvalidRecord(
                crate::memory::staged::StagedBatchError::SecretDetected { op_index: 0, class }
                    .to_string(),
            ));
        }
        // §10.1: capture the quarantine decision AT PROPOSE TIME. The taint
        // tracker is in-memory and session-sticky; re-deriving when the
        // steward dreams would both under-quarantine (tracker restart,
        // reset boundary, eviction) and over-quarantine (identity tainted
        // later by an unrelated ingestion). The persisted fact makes the
        // steward's accept downgrade deterministic shell law.
        let taint = self.gate().and_then(|gate| {
            gate.quarantine_reason(&author, StagedBatchKind::FreshWrite, &record.evidence)
        });
        if let Some(reason) = taint.as_deref() {
            tracing::warn!(
                realm = scope.realm(),
                author = ?author,
                reason,
                "agent memory: proposal from tainted context recorded as tainted; a plain \
                 steward accept will downgrade to an operator gate"
            );
        }
        let proposal_id = mint_token("prop");
        self.with_realm_conn(scope.realm(), |conn| {
            conn.execute(
                "INSERT INTO proposals (proposal_id, scope_kind, scope_key, record, author, \
                 status, created_at_ms, taint) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7)",
                params![
                    proposal_id,
                    scope.kind_str(),
                    scope.key(),
                    json_string(&record)?,
                    json_string(&author)?,
                    now_ms() as i64,
                    taint,
                ],
            )
            .map_err(sql_err)?;
            Ok(())
        })?;
        Ok(proposal_id)
    }

    fn stage_blocking(&self, batch: StagedMutationBatch) -> Result<StageToken, AgentMemoryError> {
        let realm = batch.realm.clone();
        let resolver = self.resolver();
        self.with_realm_conn(&realm, |conn| {
            {
                let view = ConnBatchView {
                    conn,
                    realm: &batch.realm,
                };
                validate_batch(
                    &batch,
                    &view,
                    DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS,
                    now_ms(),
                )
                .map_err(|err| AgentMemoryError::InvalidRecord(err.to_string()))?;
            }
            check_verified_retier_evidence(conn, &batch, resolver.as_deref())?;
            let token = mint_token("stage");
            conn.execute(
                "INSERT INTO stage (token, batch, created_at_ms) VALUES (?1, ?2, ?3)",
                params![token, json_string(&batch)?, now_ms() as i64],
            )
            .map_err(sql_err)?;
            Ok(StageToken {
                realm: realm.clone(),
                token,
            })
        })
    }

    fn commit_blocking(&self, token: StageToken) -> Result<CommitReceipt, AgentMemoryError> {
        let gate = self.gate();
        let resolver = self.resolver();
        let events = self.events();
        self.with_realm_conn(&token.realm, |conn| {
            let batch_json: Option<String> = conn
                .query_row(
                    "SELECT batch FROM stage WHERE token = ?1",
                    params![token.token],
                    |row| row.get(0),
                )
                .optional()
                .map_err(sql_err)?;
            let Some(batch_json) = batch_json else {
                return Err(AgentMemoryError::InvalidRecord(format!(
                    "unknown or expired stage token '{}'",
                    token.token
                )));
            };
            let batch: StagedMutationBatch = serde_json::from_str(&batch_json)
                .map_err(|err| AgentMemoryError::Parse(err.to_string()))?;
            check_verified_retier_evidence(conn, &batch, resolver.as_deref())?;
            apply_batch_tx(
                conn,
                &batch,
                gate.as_deref(),
                events.as_deref(),
                &token.token,
                now_ms(),
            )
        })
    }

    fn known_realms(&self) -> Result<Vec<String>, AgentMemoryError> {
        let entries =
            fs::read_dir(&self.root).map_err(|err| AgentMemoryError::Io(err.to_string()))?;
        let mut realms = Vec::new();
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "sqlite3")
                && let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
            {
                realms.push(decode_path_segment(stem));
            }
        }
        realms.sort();
        Ok(realms)
    }
}

// ---- provider trait implementations ----

#[async_trait]
impl AgentMemoryProvider for SqliteAgentMemoryStore {
    async fn recall(
        &self,
        request: AgentMemoryRecallRequest,
    ) -> Result<Vec<AgentMemoryRecord>, AgentMemoryError> {
        let store = self.clone();
        run_blocking(move || store.recall_blocking(request)).await
    }

    fn supports_remember(&self) -> bool {
        true
    }

    async fn remember(
        &self,
        realm: &str,
        identity: &AgentIdentity,
        memory: NewAgentMemory,
    ) -> Result<AgentMemoryRecord, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let identity = identity.clone();
        run_blocking(move || store.remember_blocking(&realm, &identity, memory)).await
    }

    fn supports_forget(&self) -> bool {
        true
    }

    async fn forget(
        &self,
        realm: &str,
        identity: &AgentIdentity,
        memory_id: &str,
    ) -> Result<AgentMemoryForgetResult, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let identity = identity.clone();
        let memory_id = memory_id.to_string();
        run_blocking(move || store.forget_blocking(&realm, &identity, &memory_id)).await
    }

    async fn manifest(
        &self,
        scopes: &[MemoryScope],
        tier: ManifestTier,
    ) -> Result<Vec<RecordMeta>, AgentMemoryError> {
        let store = self.clone();
        let scopes = scopes.to_vec();
        run_blocking(move || store.manifest_blocking(&scopes, tier)).await
    }

    fn supports_manifest(&self) -> bool {
        true
    }

    async fn supersede(
        &self,
        scope: &MemoryScope,
        prior: &str,
        record: NewMemoryRecord,
    ) -> Result<MemoryId, AgentMemoryError> {
        let store = self.clone();
        let scope = scope.clone();
        let prior = prior.to_string();
        run_blocking(move || store.supersede_blocking(&scope, &prior, record)).await
    }

    fn supports_supersede(&self) -> bool {
        true
    }

    async fn mark_usage(
        &self,
        ids: &[MemoryId],
        event: UsageEvent,
    ) -> Result<(), AgentMemoryError> {
        let store = self.clone();
        let ids = ids.to_vec();
        run_blocking(move || store.mark_usage_blocking(&ids, event)).await
    }

    async fn log_injections(
        &self,
        realm: &str,
        entries: &[InjectionLogEntry],
    ) -> Result<(), AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let entries = entries.to_vec();
        run_blocking(move || store.log_injections_blocking(&realm, &entries)).await
    }

    async fn propose(
        &self,
        scope: &MemoryScope,
        record: NewMemoryRecord,
        author: MemoryAuthor,
    ) -> Result<ProposalId, AgentMemoryError> {
        let store = self.clone();
        let scope = scope.clone();
        run_blocking(move || store.propose_blocking(&scope, record, author)).await
    }

    fn supports_propose(&self) -> bool {
        true
    }

    async fn remember_authored(
        &self,
        scope: &MemoryScope,
        record: NewMemoryRecord,
        author: MemoryAuthor,
    ) -> Result<AuthoredWriteReceipt, AgentMemoryError> {
        let store = self.clone();
        let scope = scope.clone();
        run_blocking(move || store.remember_authored_blocking(&scope, record, author)).await
    }

    async fn supersede_authored(
        &self,
        scope: &MemoryScope,
        prior: &str,
        record: NewMemoryRecord,
        author: MemoryAuthor,
    ) -> Result<AuthoredWriteReceipt, AgentMemoryError> {
        let store = self.clone();
        let scope = scope.clone();
        let prior = prior.to_string();
        run_blocking(move || store.supersede_with_author_blocking(&scope, &prior, record, author))
            .await
    }

    async fn forget_authored(
        &self,
        scope: &MemoryScope,
        memory_id: &str,
        author: MemoryAuthor,
    ) -> Result<AgentMemoryForgetResult, AgentMemoryError> {
        let memory_id = memory_id.trim().to_string();
        if memory_id.is_empty() {
            return Err(AgentMemoryError::InvalidRecord(
                "memory_id must not be empty".to_string(),
            ));
        }
        let store = self.clone();
        let scope = scope.clone();
        run_blocking(move || store.forget_in_scope_blocking(&scope, &memory_id, author)).await
    }

    fn supports_authored_writes(&self) -> bool {
        true
    }
}

#[async_trait]
impl StagedMemoryStore for SqliteAgentMemoryStore {
    async fn stage(&self, batch: StagedMutationBatch) -> Result<StageToken, AgentMemoryError> {
        let store = self.clone();
        run_blocking(move || store.stage_blocking(batch)).await
    }

    async fn commit(&self, token: StageToken) -> Result<CommitReceipt, AgentMemoryError> {
        let store = self.clone();
        run_blocking(move || store.commit_blocking(token)).await
    }
}

// ---- steward read/write surface (§8.5) ----

/// Per-scope store overview row for the dream's orient phase (§8.5) and
/// the P3b console Memory panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeOverview {
    pub scope: MemoryScope,
    pub active: u64,
    pub quarantined: u64,
    pub superseded: u64,
    pub tombstoned: u64,
    pub body_bytes: u64,
}

/// One pending (or held) mob/operator-scope proposal awaiting a dream
/// verdict (§8.5 promotion).
#[derive(Debug, Clone, PartialEq)]
pub struct PendingProposal {
    pub proposal_id: ProposalId,
    pub scope: MemoryScope,
    pub record: NewMemoryRecord,
    pub author: MemoryAuthor,
    pub status: String,
    pub created_at_ms: u64,
    /// §10.1 propose-time taint fact: `Some(reason)` when the write gate
    /// would have quarantined this author at propose time. A plain steward
    /// "accept" on a tainted proposal downgrades to an operator gate.
    pub taint: Option<String>,
}

/// One retired identity awaiting an exit-interview harvest (§8.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingHarvest {
    pub identity: String,
    pub session_key: Option<String>,
    pub cause: String,
    pub retired_at_ms: u64,
}

/// One gated quarantine-promotion (§10.2): the staged batch commits only on
/// gating approval.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingPromotion {
    pub pending_id: String,
    pub stage_token: String,
    pub record_id: MemoryId,
    pub scope_kind: String,
    pub scope_key: String,
    pub rationale: Option<String>,
    pub status: String,
    pub created_at_ms: u64,
}

impl SqliteAgentMemoryStore {
    /// The per-scope retention floors this store warns against (§7.3);
    /// rendered into the dream's orient overview as floor pressure.
    pub fn scope_floors(&self) -> (usize, usize) {
        (self.scope_floor_records, self.scope_floor_bytes)
    }

    /// Per-scope counts and byte totals for a realm — the orient phase's
    /// one cheap aggregate.
    pub async fn scope_overview(
        &self,
        realm: &str,
    ) -> Result<Vec<ScopeOverview>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT scope_kind, scope_key, status_kind, COUNT(*), \
                         COALESCE(SUM(LENGTH(body)), 0) FROM records \
                         GROUP BY scope_kind, scope_key, status_kind",
                    )
                    .map_err(sql_err)?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    })
                    .map_err(sql_err)?;
                let mut by_scope: HashMap<(String, String), ScopeOverview> = HashMap::new();
                for row in rows {
                    let (scope_kind, scope_key, status_kind, count, bytes) =
                        row.map_err(sql_err)?;
                    let scope = scope_from_parts(&scope_kind, &scope_key, &realm)?;
                    let entry =
                        by_scope
                            .entry((scope_kind, scope_key))
                            .or_insert_with(|| ScopeOverview {
                                scope,
                                active: 0,
                                quarantined: 0,
                                superseded: 0,
                                tombstoned: 0,
                                body_bytes: 0,
                            });
                    match status_kind.as_str() {
                        "active" => entry.active = count as u64,
                        "quarantined" => entry.quarantined = count as u64,
                        "superseded" => entry.superseded = count as u64,
                        "tombstoned" => entry.tombstoned = count as u64,
                        _ => {}
                    }
                    entry.body_bytes += bytes as u64;
                }
                let mut overview: Vec<ScopeOverview> = by_scope.into_values().collect();
                overview.sort_by(|a, b| a.scope.cmp(&b.scope));
                Ok(overview)
            })
        })
        .await
    }

    /// Pending/held proposals, oldest first (§8.5 promotion queue).
    pub async fn pending_proposals(
        &self,
        realm: &str,
        limit: usize,
    ) -> Result<Vec<PendingProposal>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT proposal_id, scope_kind, scope_key, record, author, status, \
                         created_at_ms, taint FROM proposals WHERE status IN ('pending', 'held') \
                         ORDER BY created_at_ms ASC LIMIT ?1",
                    )
                    .map_err(sql_err)?;
                let rows = stmt
                    .query_map(params![limit as i64], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, String>(5)?,
                            row.get::<_, i64>(6)?,
                            row.get::<_, Option<String>>(7)?,
                        ))
                    })
                    .map_err(sql_err)?;
                let mut proposals = Vec::new();
                for row in rows {
                    let (
                        proposal_id,
                        scope_kind,
                        scope_key,
                        record,
                        author,
                        status,
                        created,
                        taint,
                    ) = row.map_err(sql_err)?;
                    proposals.push(PendingProposal {
                        proposal_id,
                        scope: scope_from_parts(&scope_kind, &scope_key, &realm)?,
                        record: serde_json::from_str(&record)
                            .map_err(|err| AgentMemoryError::Parse(err.to_string()))?,
                        author: serde_json::from_str(&author)
                            .map_err(|err| AgentMemoryError::Parse(err.to_string()))?,
                        status,
                        created_at_ms: created as u64,
                        taint,
                    });
                }
                Ok(proposals)
            })
        })
        .await
    }

    /// Record a dream verdict on a proposal: `accepted`, `rejected`, or
    /// `held` (held stays in the pending queue for the next dream).
    pub async fn set_proposal_status(
        &self,
        realm: &str,
        proposal_id: &str,
        status: &str,
    ) -> Result<(), AgentMemoryError> {
        if !matches!(status, "accepted" | "rejected" | "held" | "pending") {
            return Err(AgentMemoryError::InvalidRecord(format!(
                "unknown proposal status '{status}'"
            )));
        }
        let store = self.clone();
        let realm = realm.to_string();
        let proposal_id = proposal_id.to_string();
        let status = status.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                let updated = conn
                    .execute(
                        "UPDATE proposals SET status = ?1 WHERE proposal_id = ?2",
                        params![status, proposal_id],
                    )
                    .map_err(sql_err)?;
                if updated == 0 {
                    return Err(AgentMemoryError::InvalidRecord(format!(
                        "unknown proposal '{proposal_id}'"
                    )));
                }
                Ok(())
            })
        })
        .await
    }

    /// Quarantined records, newest first — the dream's review queue (§8.5).
    /// The steward is the one stage that reads these bodies wholesale; the
    /// caller renders them defanged.
    pub async fn quarantined_records(
        &self,
        realm: &str,
        limit: usize,
    ) -> Result<Vec<super::records::MemoryRecord>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                let mut stmt = conn
                    .prepare(&format!(
                        "SELECT {RECORD_COLUMNS} FROM records \
                         WHERE status_kind = 'quarantined' \
                         ORDER BY created_at_ms DESC LIMIT ?1"
                    ))
                    .map_err(sql_err)?;
                let rows = stmt
                    .query_map(params![limit as i64], row_to_record_row)
                    .map_err(sql_err)?;
                let mut records = Vec::new();
                for row in rows {
                    records.push(row.map_err(sql_err)?.into_record(&realm)?);
                }
                Ok(records)
            })
        })
        .await
    }

    /// Records by id, any status — the gather phase's bounded body fetch.
    /// Missing ids are skipped (the model may cite stale ids).
    pub async fn records_by_ids(
        &self,
        realm: &str,
        ids: &[String],
    ) -> Result<Vec<super::records::MemoryRecord>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let ids = ids.to_vec();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                let mut records = Vec::new();
                for id in &ids {
                    if let Some(record) = load_record(conn, &realm, id)? {
                        records.push(record);
                    }
                }
                Ok(records)
            })
        })
        .await
    }

    /// Most recently updated records in a realm, any scope, active or
    /// quarantined — the gather phase filters (e.g. recent distillates by
    /// author) host-side.
    pub async fn recent_records(
        &self,
        realm: &str,
        limit: usize,
    ) -> Result<Vec<super::records::MemoryRecord>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                let mut stmt = conn
                    .prepare(&format!(
                        "SELECT {RECORD_COLUMNS} FROM records \
                         WHERE status_kind IN ('active', 'quarantined') \
                         ORDER BY updated_at_ms DESC LIMIT ?1"
                    ))
                    .map_err(sql_err)?;
                let rows = stmt
                    .query_map(params![limit as i64], row_to_record_row)
                    .map_err(sql_err)?;
                let mut records = Vec::new();
                for row in rows {
                    records.push(row.map_err(sql_err)?.into_record(&realm)?);
                }
                Ok(records)
            })
        })
        .await
    }

    /// Record a retired identity for the next dream's exit-interview
    /// harvest (§8.5). Idempotent per (identity, retired_at_ms).
    pub async fn record_pending_harvest(
        &self,
        realm: &str,
        identity: &str,
        session_key: Option<&str>,
        cause: &str,
    ) -> Result<(), AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let identity = identity.to_string();
        let session_key = session_key.map(str::to_string);
        let cause = cause.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO pending_harvests \
                     (identity, session_key, cause, retired_at_ms, status) \
                     VALUES (?1, ?2, ?3, ?4, 'pending')",
                    params![identity, session_key, cause, now_ms() as i64],
                )
                .map_err(sql_err)?;
                Ok(())
            })
        })
        .await
    }

    /// Pending exit-interview harvests, oldest first.
    pub async fn pending_harvests(
        &self,
        realm: &str,
        limit: usize,
    ) -> Result<Vec<PendingHarvest>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT identity, session_key, cause, retired_at_ms FROM \
                         pending_harvests WHERE status = 'pending' \
                         ORDER BY retired_at_ms ASC LIMIT ?1",
                    )
                    .map_err(sql_err)?;
                let rows = stmt
                    .query_map(params![limit as i64], |row| {
                        Ok(PendingHarvest {
                            identity: row.get(0)?,
                            session_key: row.get(1)?,
                            cause: row.get(2)?,
                            retired_at_ms: row.get::<_, i64>(3)? as u64,
                        })
                    })
                    .map_err(sql_err)?;
                let mut harvests = Vec::new();
                for row in rows {
                    harvests.push(row.map_err(sql_err)?);
                }
                Ok(harvests)
            })
        })
        .await
    }

    /// Mark one exit-interview harvest done.
    pub async fn mark_harvest_complete(
        &self,
        realm: &str,
        identity: &str,
        retired_at_ms: u64,
    ) -> Result<(), AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let identity = identity.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                conn.execute(
                    "UPDATE pending_harvests SET status = 'harvested' \
                     WHERE identity = ?1 AND retired_at_ms = ?2",
                    params![identity, retired_at_ms as i64],
                )
                .map_err(sql_err)?;
                Ok(())
            })
        })
        .await
    }

    /// Record a gated quarantine-promotion: gating `pending_id` → staged
    /// batch token (§10.2). Only a gating approval commits the token.
    pub async fn record_pending_promotion(
        &self,
        realm: &str,
        promotion: PendingPromotion,
    ) -> Result<(), AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                conn.execute(
                    "INSERT INTO pending_promotions (pending_id, stage_token, record_id, \
                     scope_kind, scope_key, rationale, status, created_at_ms) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        promotion.pending_id,
                        promotion.stage_token,
                        promotion.record_id,
                        promotion.scope_kind,
                        promotion.scope_key,
                        promotion.rationale,
                        promotion.status,
                        promotion.created_at_ms as i64,
                    ],
                )
                .map_err(sql_err)?;
                Ok(())
            })
        })
        .await
    }

    /// Look up a still-pending gated promotion by its gating pending id.
    pub async fn pending_promotion_by_id(
        &self,
        realm: &str,
        pending_id: &str,
    ) -> Result<Option<PendingPromotion>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let pending_id = pending_id.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                conn.query_row(
                    "SELECT pending_id, stage_token, record_id, scope_kind, scope_key, \
                     rationale, status, created_at_ms FROM pending_promotions \
                     WHERE pending_id = ?1 AND status = 'pending'",
                    params![pending_id],
                    |row| {
                        Ok(PendingPromotion {
                            pending_id: row.get(0)?,
                            stage_token: row.get(1)?,
                            record_id: row.get(2)?,
                            scope_kind: row.get(3)?,
                            scope_key: row.get(4)?,
                            rationale: row.get(5)?,
                            status: row.get(6)?,
                            created_at_ms: row.get::<_, i64>(7)? as u64,
                        })
                    },
                )
                .optional()
                .map_err(sql_err)
            })
        })
        .await
    }

    /// All still-pending gated promotions (dream-start reconciliation).
    pub async fn pending_promotions(
        &self,
        realm: &str,
    ) -> Result<Vec<PendingPromotion>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT pending_id, stage_token, record_id, scope_kind, scope_key, \
                         rationale, status, created_at_ms FROM pending_promotions \
                         WHERE status = 'pending' ORDER BY created_at_ms ASC",
                    )
                    .map_err(sql_err)?;
                let rows = stmt
                    .query_map([], |row| {
                        Ok(PendingPromotion {
                            pending_id: row.get(0)?,
                            stage_token: row.get(1)?,
                            record_id: row.get(2)?,
                            scope_kind: row.get(3)?,
                            scope_key: row.get(4)?,
                            rationale: row.get(5)?,
                            status: row.get(6)?,
                            created_at_ms: row.get::<_, i64>(7)? as u64,
                        })
                    })
                    .map_err(sql_err)?;
                let mut promotions = Vec::new();
                for row in rows {
                    promotions.push(row.map_err(sql_err)?);
                }
                Ok(promotions)
            })
        })
        .await
    }

    /// Resolve a gated promotion: `committed`, `denied`, or `expired`.
    pub async fn resolve_pending_promotion(
        &self,
        realm: &str,
        pending_id: &str,
        status: &str,
    ) -> Result<(), AgentMemoryError> {
        if !matches!(status, "committed" | "denied" | "expired") {
            return Err(AgentMemoryError::InvalidRecord(format!(
                "unknown promotion resolution '{status}'"
            )));
        }
        let store = self.clone();
        let realm = realm.to_string();
        let pending_id = pending_id.to_string();
        let status = status.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                conn.execute(
                    "UPDATE pending_promotions SET status = ?1, resolved_at_ms = ?2 \
                     WHERE pending_id = ?3",
                    params![status, now_ms() as i64, pending_id],
                )
                .map_err(sql_err)?;
                Ok(())
            })
        })
        .await
    }

    /// Re-key a gated promotion after a gating escalation minted a
    /// successor pending entry.
    pub async fn rekey_pending_promotion(
        &self,
        realm: &str,
        old_pending_id: &str,
        new_pending_id: &str,
    ) -> Result<(), AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let old_pending_id = old_pending_id.to_string();
        let new_pending_id = new_pending_id.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                conn.execute(
                    "UPDATE pending_promotions SET pending_id = ?1 WHERE pending_id = ?2",
                    params![new_pending_id, old_pending_id],
                )
                .map_err(sql_err)?;
                Ok(())
            })
        })
        .await
    }

    /// Discard a staged-but-uncommitted batch (denied/expired gated
    /// promotions; §8.5 crash semantics keep this safe — an unapplied stage
    /// row is never visible).
    pub async fn discard_stage(&self, token: StageToken) -> Result<(), AgentMemoryError> {
        let store = self.clone();
        run_blocking(move || {
            store.with_realm_conn(&token.realm, |conn| {
                conn.execute("DELETE FROM stage WHERE token = ?1", params![token.token])
                    .map_err(sql_err)?;
                Ok(())
            })
        })
        .await
    }
}

// ---- console Memory panel read surface (§9.3, P3b) ----

/// One page of panel records: strictly-descending `(updated_at_ms,
/// memory_id)` keyset pagination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelRecordsPage {
    pub records: Vec<super::records::MemoryRecord>,
    /// Pass back as `cursor` to continue; `None` when exhausted.
    pub next_cursor: Option<(u64, String)>,
}

/// One steward dream run reconstructed from its audit rows (every committed
/// op records its `Steward { run_id }` author in the audit `detail`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DreamRunAudit {
    pub run_id: String,
    pub first_op_at_ms: u64,
    pub last_op_at_ms: u64,
    pub ops: u64,
    /// op kind → count (create/supersede/tombstone/retier/set_rank).
    pub op_kinds: std::collections::BTreeMap<String, u64>,
    /// Ops that landed quarantined at the write seam.
    pub quarantined_ops: u64,
    /// Bounded sample of touched record ids, newest first.
    pub memory_ids: Vec<String>,
    /// Bounded sample of op rationales, newest first.
    pub rationales: Vec<String>,
}

/// Bounds for [`SqliteAgentMemoryStore::dream_history`]: audit rows scanned
/// per call and per-run sample sizes. The panel is a summary surface, not a
/// full audit export.
const DREAM_HISTORY_SCAN_ROWS: usize = 5_000;
const DREAM_HISTORY_ID_SAMPLE: usize = 12;
const DREAM_HISTORY_RATIONALE_SAMPLE: usize = 6;

impl SqliteAgentMemoryStore {
    /// Realms with a store file on disk (panel realm picker).
    pub async fn panel_realms(&self) -> Result<Vec<String>, AgentMemoryError> {
        let store = self.clone();
        run_blocking(move || store.known_realms()).await
    }

    /// One record by id, any status.
    pub async fn record_by_id(
        &self,
        realm: &str,
        memory_id: &str,
    ) -> Result<Option<super::records::MemoryRecord>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let memory_id = memory_id.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| load_record(conn, &realm, &memory_id))
        })
        .await
    }

    /// Panel record listing: optional scope/status filters, newest-updated
    /// first, keyset cursor. Any status is visible here — the panel is an
    /// inspection surface and renders status explicitly.
    pub async fn records_page(
        &self,
        realm: &str,
        scope_kind: Option<&str>,
        scope_key: Option<&str>,
        status_kind: Option<&str>,
        limit: usize,
        cursor: Option<(u64, String)>,
    ) -> Result<PanelRecordsPage, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let scope_kind = scope_kind.map(str::to_string);
        let scope_key = scope_key.map(str::to_string);
        let status_kind = status_kind.map(str::to_string);
        let limit = limit.max(1);
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                let mut clauses: Vec<String> = Vec::new();
                let mut values: Vec<rusqlite::types::Value> = Vec::new();
                if let Some(kind) = &scope_kind {
                    values.push(kind.clone().into());
                    clauses.push(format!("scope_kind = ?{}", values.len()));
                }
                if let Some(key) = &scope_key {
                    values.push(key.clone().into());
                    clauses.push(format!("scope_key = ?{}", values.len()));
                }
                if let Some(status) = &status_kind {
                    values.push(status.clone().into());
                    clauses.push(format!("status_kind = ?{}", values.len()));
                }
                if let Some((after_ms, after_id)) = &cursor {
                    values.push((*after_ms as i64).into());
                    let ms_slot = values.len();
                    values.push(after_id.clone().into());
                    let id_slot = values.len();
                    clauses.push(format!(
                        "(updated_at_ms < ?{ms_slot} OR (updated_at_ms = ?{ms_slot} \
                         AND memory_id < ?{id_slot}))"
                    ));
                }
                let where_sql = if clauses.is_empty() {
                    String::new()
                } else {
                    format!("WHERE {}", clauses.join(" AND "))
                };
                values.push(((limit + 1) as i64).into());
                let sql = format!(
                    "SELECT {RECORD_COLUMNS} FROM records {where_sql} \
                     ORDER BY updated_at_ms DESC, memory_id DESC LIMIT ?{}",
                    values.len()
                );
                let mut stmt = conn.prepare(&sql).map_err(sql_err)?;
                let rows = stmt
                    .query_map(rusqlite::params_from_iter(values), row_to_record_row)
                    .map_err(sql_err)?;
                let mut records = Vec::new();
                for row in rows {
                    records.push(row.map_err(sql_err)?.into_record(&realm)?);
                }
                let next_cursor = if records.len() > limit {
                    records.truncate(limit);
                    records
                        .last()
                        .map(|record| (record.updated_at_ms, record.id.clone()))
                } else {
                    None
                };
                Ok(PanelRecordsPage {
                    records,
                    next_cursor,
                })
            })
        })
        .await
    }

    /// Supersede lineage around one record, oldest first: ancestors via the
    /// `supersedes` pointer, the record itself, then committed successors
    /// via the `Superseded { by }` status link. When the tip has no
    /// committed successor, records *claiming* to supersede it (e.g. a
    /// quarantined supersede that left the prior active, §10.1) are
    /// appended without recursing — claims are visible but never extend
    /// the walk. Bounded by `max_len`, cycle-safe.
    pub async fn supersede_chain(
        &self,
        realm: &str,
        memory_id: &str,
        max_len: usize,
    ) -> Result<Vec<super::records::MemoryRecord>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let memory_id = memory_id.to_string();
        let max_len = max_len.max(1);
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                let Some(origin) = load_record(conn, &realm, &memory_id)? else {
                    return Ok(Vec::new());
                };
                let mut seen: std::collections::BTreeSet<String> =
                    std::collections::BTreeSet::from([origin.id.clone()]);
                let mut ancestors: Vec<super::records::MemoryRecord> = Vec::new();
                let mut parent_id = origin.supersedes.clone();
                while let Some(id) = parent_id {
                    if ancestors.len() + 1 >= max_len || !seen.insert(id.clone()) {
                        break;
                    }
                    let Some(parent) = load_record(conn, &realm, &id)? else {
                        break;
                    };
                    parent_id = parent.supersedes.clone();
                    ancestors.push(parent);
                }
                ancestors.reverse();
                let mut chain = ancestors;
                chain.push(origin);
                loop {
                    if chain.len() >= max_len {
                        return Ok(chain);
                    }
                    let tip = chain.last().unwrap_or_else(|| unreachable!());
                    let successor_id = match &tip.status {
                        super::records::RecordStatus::Superseded { by } => Some(by.clone()),
                        _ => None,
                    };
                    match successor_id {
                        Some(id) => {
                            if !seen.insert(id.clone()) {
                                return Ok(chain);
                            }
                            let Some(successor) = load_record(conn, &realm, &id)? else {
                                return Ok(chain);
                            };
                            chain.push(successor);
                        }
                        None => {
                            // Trailing claimants: visible, not walked.
                            let tip_id = tip.id.clone();
                            let mut stmt = conn
                                .prepare(&format!(
                                    "SELECT {RECORD_COLUMNS} FROM records \
                                     WHERE supersedes = ?1 ORDER BY created_at_ms ASC"
                                ))
                                .map_err(sql_err)?;
                            let rows = stmt
                                .query_map(params![tip_id], row_to_record_row)
                                .map_err(sql_err)?;
                            for row in rows {
                                if chain.len() >= max_len {
                                    break;
                                }
                                let claimant = row.map_err(sql_err)?.into_record(&realm)?;
                                if seen.insert(claimant.id.clone()) {
                                    chain.push(claimant);
                                }
                            }
                            return Ok(chain);
                        }
                    }
                }
            })
        })
        .await
    }

    /// Newest-first injection-ledger rows for one record (panel usage view).
    pub async fn injection_log_for_record(
        &self,
        realm: &str,
        record_id: &str,
        limit: usize,
    ) -> Result<Vec<InjectionLogEntry>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let record_id = record_id.to_string();
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT record_id, identity, session_key, surface, at_ms \
                         FROM injections WHERE record_id = ?1 \
                         ORDER BY at_ms DESC, injection_id DESC LIMIT ?2",
                    )
                    .map_err(sql_err)?;
                let rows = stmt
                    .query_map(params![record_id, limit as i64], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, Option<String>>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    })
                    .map_err(sql_err)?;
                let mut entries = Vec::new();
                for row in rows {
                    let (record_id, identity, session_key, surface, at_ms) =
                        row.map_err(sql_err)?;
                    let surface = InjectionSurface::parse(&surface).ok_or_else(|| {
                        AgentMemoryError::Parse(format!("unknown injection surface '{surface}'"))
                    })?;
                    entries.push(InjectionLogEntry {
                        record_id,
                        identity,
                        session_key,
                        surface,
                        at_ms: at_ms as u64,
                    });
                }
                Ok(entries)
            })
        })
        .await
    }

    /// Dream-run summaries reconstructed from steward audit rows, newest
    /// run first. Bounded scan ([`DREAM_HISTORY_SCAN_ROWS`]); runs older
    /// than the scan window fall off the panel, which is acceptable for a
    /// history summary surface.
    pub async fn dream_history(
        &self,
        realm: &str,
        max_runs: usize,
    ) -> Result<Vec<DreamRunAudit>, AgentMemoryError> {
        let store = self.clone();
        let realm = realm.to_string();
        let max_runs = max_runs.max(1);
        run_blocking(move || {
            store.with_realm_conn(&realm, |conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT op_kind, memory_id, detail, applied_at_ms FROM audit \
                         ORDER BY applied_at_ms DESC, audit_id DESC LIMIT ?1",
                    )
                    .map_err(sql_err)?;
                let rows = stmt
                    .query_map(params![DREAM_HISTORY_SCAN_ROWS as i64], |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    })
                    .map_err(sql_err)?;
                let mut order: Vec<String> = Vec::new();
                let mut runs: HashMap<String, DreamRunAudit> = HashMap::new();
                for row in rows {
                    let (op_kind, memory_id, detail, applied_at_ms) = row.map_err(sql_err)?;
                    let detail: serde_json::Value =
                        serde_json::from_str(&detail).unwrap_or_default();
                    let author = detail.get("author");
                    let is_steward = author
                        .and_then(|author| author.get("author"))
                        .and_then(serde_json::Value::as_str)
                        == Some("steward");
                    if !is_steward {
                        continue;
                    }
                    let Some(run_id) = author
                        .and_then(|author| author.get("run_id"))
                        .and_then(serde_json::Value::as_str)
                    else {
                        continue;
                    };
                    if !runs.contains_key(run_id) {
                        if runs.len() >= max_runs {
                            continue;
                        }
                        order.push(run_id.to_string());
                    }
                    let run = runs
                        .entry(run_id.to_string())
                        .or_insert_with(|| DreamRunAudit {
                            run_id: run_id.to_string(),
                            first_op_at_ms: applied_at_ms as u64,
                            last_op_at_ms: applied_at_ms as u64,
                            ..DreamRunAudit::default()
                        });
                    run.ops += 1;
                    run.first_op_at_ms = run.first_op_at_ms.min(applied_at_ms as u64);
                    run.last_op_at_ms = run.last_op_at_ms.max(applied_at_ms as u64);
                    *run.op_kinds.entry(op_kind).or_insert(0) += 1;
                    if !detail
                        .get("quarantined")
                        .map(serde_json::Value::is_null)
                        .unwrap_or(true)
                    {
                        run.quarantined_ops += 1;
                    }
                    if let Some(memory_id) = memory_id
                        && run.memory_ids.len() < DREAM_HISTORY_ID_SAMPLE
                    {
                        run.memory_ids.push(memory_id);
                    }
                    if let Some(rationale) =
                        detail.get("rationale").and_then(serde_json::Value::as_str)
                        && !rationale.is_empty()
                        && run.rationales.len() < DREAM_HISTORY_RATIONALE_SAMPLE
                    {
                        run.rationales.push(rationale.to_string());
                    }
                }
                Ok(order
                    .into_iter()
                    .filter_map(|run_id| runs.remove(&run_id))
                    .collect())
            })
        })
        .await
    }
}

#[async_trait]
impl crate::memory::distiller::TombstoneSource for SqliteAgentMemoryStore {
    /// Recent tombstones for the Distiller's pre-injected "never re-create
    /// these" list (§8.4). The mechanical backstop for exact recreation is
    /// the staged validator's content-hash check; this list closes the
    /// paraphrase gap at the prompt level.
    async fn recent_tombstones(
        &self,
        scope: &MemoryScope,
        since_ms: u64,
        limit: usize,
    ) -> Result<Vec<crate::memory::distiller::TombstoneMeta>, AgentMemoryError> {
        let store = self.clone();
        let scope = scope.clone();
        run_blocking(move || {
            store.with_realm_conn(&scope.realm().to_string(), |conn| {
                let mut statement = conn
                    .prepare(
                        "SELECT title, kind, tombstoned_at_ms FROM records \
                         WHERE scope_kind = ?1 AND scope_key = ?2 \
                           AND status_kind = 'tombstoned' AND tombstoned_at_ms >= ?3 \
                         ORDER BY tombstoned_at_ms DESC LIMIT ?4",
                    )
                    .map_err(sql_err)?;
                let rows = statement
                    .query_map(
                        params![scope.kind_str(), scope.key(), since_ms as i64, limit as i64],
                        |row| {
                            Ok((
                                row.get::<_, String>(0)?,
                                row.get::<_, String>(1)?,
                                row.get::<_, i64>(2)?,
                            ))
                        },
                    )
                    .map_err(sql_err)?;
                let mut tombstones = Vec::new();
                for row in rows {
                    let (title, kind, tombstoned_at_ms) = row.map_err(sql_err)?;
                    let kind = MemoryKind::parse(&kind).ok_or_else(|| {
                        AgentMemoryError::Parse(format!("unknown record kind '{kind}'"))
                    })?;
                    tombstones.push(crate::memory::distiller::TombstoneMeta {
                        title,
                        kind,
                        tombstoned_at_ms: tombstoned_at_ms as u64,
                    });
                }
                Ok(tombstones)
            })
        })
        .await
    }
}

/// Body fetch for selector-chosen ids (§8.3): a plain by-id read over the
/// composed scopes, wire-compat projected, returned in `ids` order. Only
/// active records in the requested scopes qualify — the selector judged a
/// manifest of exactly those.
#[async_trait]
impl crate::memory::selector::SelectedRecordFetch for SqliteAgentMemoryStore {
    async fn fetch_records(
        &self,
        scopes: &[MemoryScope],
        ids: &[String],
    ) -> Result<Vec<AgentMemoryRecord>, AgentMemoryError> {
        let store = self.clone();
        let scopes = scopes.to_vec();
        let ids = ids.to_vec();
        run_blocking(move || {
            let mut records = Vec::new();
            for id in &ids {
                for scope in &scopes {
                    let found = store.with_realm_conn(scope.realm(), |conn| {
                        load_record(conn, scope.realm(), id)
                    })?;
                    if let Some(record) = found
                        && record.scope == *scope
                        && matches!(record.status, RecordStatus::Active)
                    {
                        records.push(project_record(record));
                        break;
                    }
                }
            }
            Ok(records)
        })
        .await
    }

    async fn fetch_records_annotated(
        &self,
        scopes: &[MemoryScope],
        ids: &[String],
    ) -> Result<Vec<crate::memory::selector::AnnotatedRecord>, AgentMemoryError> {
        let store = self.clone();
        let scopes = scopes.to_vec();
        let ids = ids.to_vec();
        run_blocking(move || {
            let mut records = Vec::new();
            for id in &ids {
                for scope in &scopes {
                    let found = store.with_realm_conn(scope.realm(), |conn| {
                        load_record(conn, scope.realm(), id)
                    })?;
                    if let Some(record) = found
                        && record.scope == *scope
                        && matches!(record.status, RecordStatus::Active)
                    {
                        // The full MemoryRecord is in hand before projection
                        // strips it — carry scope + trust so injected bodies
                        // render their §7.2 labels.
                        let provenance = Some(crate::memory::selector::RecordProvenance {
                            scope: record.scope.clone(),
                            trust: record.trust,
                        });
                        records.push(crate::memory::selector::AnnotatedRecord {
                            record: project_record(record),
                            provenance,
                        });
                        break;
                    }
                }
            }
            Ok(records)
        })
        .await
    }
}

// ---- blocking internals ----

async fn run_blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AgentMemoryError> + Send + 'static,
) -> Result<T, AgentMemoryError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| AgentMemoryError::Io(format!("agent memory task failed: {err}")))?
}

/// §10.2 P3 validator extension, enforced at the store seam (stage and
/// commit): every `Retier` to `agent_verified` requires the target record's
/// verification claim to cite at least one `EvidenceRef` that resolves
/// against the session store. No resolver wired ⇒ the P2 claim-presence
/// rule stands alone (wiring that enables the steward installs one).
fn check_verified_retier_evidence(
    conn: &Connection,
    batch: &StagedMutationBatch,
    resolver: Option<&dyn EvidenceRefResolver>,
) -> Result<(), AgentMemoryError> {
    let Some(resolver) = resolver else {
        return Ok(());
    };
    for (op_index, op) in batch.ops.iter().enumerate() {
        let StagedOp::Retier { id, trust, .. } = op else {
            continue;
        };
        if *trust != TrustTier::AgentVerified {
            continue;
        }
        let reject = |reason: String| {
            AgentMemoryError::InvalidRecord(
                super::staged::StagedBatchError::UnresolvableEvidence { op_index, reason }
                    .to_string(),
            )
        };
        let provenance: Option<String> = conn
            .query_row(
                "SELECT provenance FROM records WHERE memory_id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()
            .map_err(sql_err)?;
        let Some(provenance) = provenance else {
            // Unknown record — validate_batch already rejects this.
            continue;
        };
        let provenance: MemoryProvenance = serde_json::from_str(&provenance)
            .map_err(|err| AgentMemoryError::Parse(err.to_string()))?;
        let evidence = provenance
            .verification
            .as_ref()
            .map(|claim| claim.evidence.as_slice())
            .unwrap_or(&[]);
        if evidence.is_empty() {
            return Err(reject(
                "verification claim cites no evidence refs".to_string(),
            ));
        }
        for reference in evidence {
            resolver.resolves(reference).map_err(reject)?;
        }
    }
    Ok(())
}

/// Validates (against the live transaction) and applies a batch atomically:
/// one SQLite transaction, one audit row per op (§8.5).
///
/// `gate` is the §10.1 LLM write gate: consulted once per batch (the
/// quarantine decision is a property of the author's session/posture and of
/// the batch's cited evidence, not of individual ops — a batch with any
/// tainted evidence quarantines wholesale, conservative direction) and
/// applied to every create/supersede in the batch. `None` only for the
/// markdown import, which migrates already-accepted records rather than
/// writing new LLM output.
fn apply_batch_tx(
    conn: &mut Connection,
    batch: &StagedMutationBatch,
    gate: Option<&dyn LlmWriteGate>,
    events: Option<&dyn crate::memory::events::MemoryEventSink>,
    token: &str,
    now: u64,
) -> Result<CommitReceipt, AgentMemoryError> {
    let evidence: Vec<crate::memory::records::EvidenceRef> = batch
        .ops
        .iter()
        .flat_map(|op| match op {
            StagedOp::Create { record, .. } | StagedOp::Supersede { record, .. } => {
                record.evidence.clone()
            }
            _ => Vec::new(),
        })
        .collect();
    let quarantine =
        gate.and_then(|gate| gate.quarantine_reason(&batch.author, batch.kind, &evidence));
    if let Some(reason) = quarantine.as_deref() {
        tracing::warn!(
            realm = %batch.realm,
            author = ?batch.author,
            reason,
            "agent memory: LLM-authored write landing quarantined (write-only until review)"
        );
        if let Some(events) = events {
            events.emit(
                crate::memory::events::MemoryTimelineEvent::QuarantinedWrite {
                    realm: batch.realm.clone(),
                    author: format!("{:?}", batch.author),
                    reason: reason.to_string(),
                },
            );
        }
    }
    let tx = conn.transaction().map_err(sql_err)?;
    {
        let view = ConnBatchView {
            conn: &tx,
            realm: &batch.realm,
        };
        validate_batch(batch, &view, DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS, now)
            .map_err(|err| AgentMemoryError::InvalidRecord(err.to_string()))?;
    }
    let mut memory_ids = Vec::with_capacity(batch.ops.len());
    for (op_index, op) in batch.ops.iter().enumerate() {
        let memory_id = apply_op(&tx, batch, op, quarantine.as_deref(), now)?;
        let detail = serde_json::json!({
            "op": op.kind_str(),
            "author": batch.author,
            "rationale": op_rationale(op),
            "quarantined": quarantine,
        });
        tx.execute(
            "INSERT INTO audit (stage_token, op_index, op_kind, memory_id, detail, \
             applied_at_ms) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                token,
                op_index as i64,
                op.kind_str(),
                memory_id,
                detail.to_string(),
                now as i64,
            ],
        )
        .map_err(sql_err)?;
        memory_ids.push(memory_id);
    }
    tx.execute("DELETE FROM stage WHERE token = ?1", params![token])
        .map_err(sql_err)?;
    tx.commit().map_err(sql_err)?;
    Ok(CommitReceipt {
        token: token.to_string(),
        applied_ops: batch.ops.len(),
        memory_ids,
    })
}

fn op_rationale(op: &StagedOp) -> Option<String> {
    match op {
        StagedOp::Create { rationale, .. }
        | StagedOp::Supersede { rationale, .. }
        | StagedOp::Tombstone { rationale, .. }
        | StagedOp::Retier { rationale, .. } => rationale.clone(),
        StagedOp::SetRank { .. } => None,
    }
}

fn apply_op(
    conn: &Connection,
    batch: &StagedMutationBatch,
    op: &StagedOp,
    quarantine: Option<&str>,
    now: u64,
) -> Result<MemoryId, AgentMemoryError> {
    match op {
        StagedOp::Create {
            id,
            scope,
            record,
            trust,
            derived_from,
            created_at_ms,
            updated_at_ms,
            ..
        } => {
            let memory_id = id
                .clone()
                .unwrap_or_else(|| new_memory_id(&record.title, &record.body));
            insert_record(
                conn,
                &memory_id,
                scope,
                record,
                *trust,
                &batch.author,
                derived_from,
                None,
                None,
                None,
                quarantine,
                created_at_ms.unwrap_or(now),
                updated_at_ms.unwrap_or(now),
            )?;
            Ok(memory_id)
        }
        StagedOp::Supersede {
            id,
            prior,
            record,
            trust,
            derived_from,
            ..
        } => {
            let prior_row: (String, String, Option<i64>) = conn
                .query_row(
                    "SELECT scope_kind, scope_key, working_set_rank \
                     FROM records WHERE memory_id = ?1",
                    params![prior],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .map_err(sql_err)?;
            let scope = scope_from_parts(&prior_row.0, &prior_row.1, &batch.realm)?;
            let memory_id = id
                .clone()
                .unwrap_or_else(|| new_memory_id(&record.title, &record.body));
            // §8.3 / §7.1: the superseding record inherits the prior's rank
            // until the next dream re-ranks. rank_set_at_ms stays NULL so
            // the successor also remains in the manifest's recent slice —
            // a fresh correction is selector-visible on the next assembly.
            insert_record(
                conn,
                &memory_id,
                &scope,
                record,
                *trust,
                &batch.author,
                derived_from,
                Some(prior.clone()),
                prior_row.2,
                None,
                quarantine,
                now,
                now,
            )?;
            if quarantine.is_none() {
                conn.execute(
                    "UPDATE records SET status_kind = 'superseded', status_detail = ?1, \
                     updated_at_ms = ?2 WHERE memory_id = ?3",
                    params![memory_id, now as i64, prior],
                )
                .map_err(sql_err)?;
            } else {
                // A quarantined supersede must not retire the active prior:
                // otherwise a tainted session could silently blank a good
                // record by "updating" it. The quarantined successor keeps
                // its `supersedes` lineage edge; the steward resolves the
                // fork at review (promote → prior superseded; tombstone →
                // lineage unchanged).
                tracing::warn!(
                    prior,
                    successor = %memory_id,
                    "agent memory: quarantined supersede leaves the prior record active \
                     pending review"
                );
            }
            Ok(memory_id)
        }
        StagedOp::Tombstone { id, .. } => {
            conn.execute(
                "UPDATE records SET status_kind = 'tombstoned', status_detail = NULL, \
                 tombstoned_at_ms = ?1, updated_at_ms = ?1 WHERE memory_id = ?2",
                params![now as i64, id],
            )
            .map_err(sql_err)?;
            Ok(id.clone())
        }
        StagedOp::Retier { id, trust, .. } => {
            conn.execute(
                "UPDATE records SET trust = ?1, updated_at_ms = ?2 WHERE memory_id = ?3",
                params![trust.as_str(), now as i64, id],
            )
            .map_err(sql_err)?;
            Ok(id.clone())
        }
        StagedOp::SetRank { id, rank } => {
            // Rank is steward metadata: updated_at_ms is deliberately NOT
            // bumped, or every re-rank would flood the manifest's
            // "updated since last rank" recent slice.
            conn.execute(
                "UPDATE records SET working_set_rank = ?1, rank_set_at_ms = ?2 \
                 WHERE memory_id = ?3",
                params![rank.map(|r| r as i64), now as i64, id],
            )
            .map_err(sql_err)?;
            Ok(id.clone())
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn insert_record(
    conn: &Connection,
    memory_id: &str,
    scope: &MemoryScope,
    record: &NewMemoryRecord,
    trust: TrustTier,
    author: &MemoryAuthor,
    derived_from: &[MemoryId],
    supersedes: Option<MemoryId>,
    working_set_rank: Option<i64>,
    rank_set_at_ms: Option<i64>,
    quarantine: Option<&str>,
    created_at_ms: u64,
    updated_at_ms: u64,
) -> Result<(), AgentMemoryError> {
    let tags = normalize_tags(record.tags.clone())?;
    let provenance = MemoryProvenance {
        evidence: record.evidence.clone(),
        author: author.clone(),
        profile: None,
        verification: record.verification.clone(),
    };
    // §10.1: the gate's verdict lands as row status. Quarantined records are
    // write-only — every read surface filters on status_kind = 'active'.
    let (status_kind, status_detail) = match quarantine {
        Some(reason) => ("quarantined", Some(reason)),
        None => ("active", None),
    };
    // §10.2 durable taint: set when landing quarantined, inherited from any
    // direct ancestor (derivation source or superseded prior) that carries
    // it or currently sits quarantined. Materialized transitively at each
    // insert, so one level suffices; the validator's chain walk remains the
    // enforcement.
    let ever_quarantined = quarantine.is_some() || {
        let mut ancestors: Vec<&str> = derived_from.iter().map(String::as_str).collect();
        if let Some(prior) = supersedes.as_deref() {
            ancestors.push(prior);
        }
        ancestors_reach_quarantine(conn, &ancestors)?
    };
    conn.execute(
        "INSERT INTO records (memory_id, scope_kind, scope_key, kind, title, description, \
         body, tags, provenance, trust, status_kind, status_detail, supersedes, derived_from, \
         working_set_rank, rank_set_at_ms, content_hash, created_at_ms, updated_at_ms, \
         usage_stats, tombstoned_at_ms, ever_quarantined) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, \
         ?16, ?17, ?18, ?19, ?20, NULL, ?21)",
        params![
            memory_id,
            scope.kind_str(),
            scope.key(),
            record.kind.as_str(),
            record.title,
            record.description,
            record.body,
            json_string(&tags)?,
            json_string(&provenance)?,
            trust.as_str(),
            status_kind,
            status_detail,
            supersedes,
            json_string(&derived_from.to_vec())?,
            working_set_rank,
            rank_set_at_ms,
            content_hash(&record.title, &record.body),
            created_at_ms as i64,
            updated_at_ms as i64,
            json_string(&UsageStats::default())?,
            ever_quarantined,
        ],
    )
    .map_err(sql_err)?;
    Ok(())
}

/// One-level ancestor check backing the materialized `ever_quarantined`
/// inheritance in [`insert_record`].
fn ancestors_reach_quarantine(
    conn: &Connection,
    ancestors: &[&str],
) -> Result<bool, AgentMemoryError> {
    if ancestors.is_empty() {
        return Ok(false);
    }
    let placeholders = (1..=ancestors.len())
        .map(|slot| format!("?{slot}"))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        "SELECT 1 FROM records WHERE memory_id IN ({placeholders}) \
         AND (ever_quarantined = 1 OR status_kind = 'quarantined') LIMIT 1"
    );
    let hit: Option<i64> = conn
        .query_row(&sql, rusqlite::params_from_iter(ancestors.iter()), |row| {
            row.get(0)
        })
        .optional()
        .map_err(sql_err)?;
    Ok(hit.is_some())
}

/// Validator view over a live connection/transaction. Rows in a realm DB
/// are realm-homogeneous by construction, so the view carries the realm to
/// reconstruct full scopes for the validator's realm-confinement checks.
struct ConnBatchView<'a> {
    conn: &'a Connection,
    realm: &'a str,
}

impl StagedBatchView for ConnBatchView<'_> {
    fn record(&self, id: &str) -> Option<StagedRecordView> {
        self.conn
            .query_row(
                "SELECT scope_kind, scope_key, trust, status_kind, status_detail, supersedes, \
                 derived_from, content_hash, provenance, ever_quarantined \
                 FROM records WHERE memory_id = ?1",
                params![id],
                |row| {
                    let scope_kind: String = row.get(0)?;
                    let scope_key: String = row.get(1)?;
                    let trust: String = row.get(2)?;
                    let status_kind: String = row.get(3)?;
                    let status_detail: Option<String> = row.get(4)?;
                    let supersedes: Option<String> = row.get(5)?;
                    let derived_from: String = row.get(6)?;
                    let hash: String = row.get(7)?;
                    let provenance: String = row.get(8)?;
                    let ever_quarantined: bool = row.get(9)?;
                    Ok((
                        scope_kind,
                        scope_key,
                        trust,
                        status_kind,
                        status_detail,
                        supersedes,
                        derived_from,
                        hash,
                        provenance,
                        ever_quarantined,
                    ))
                },
            )
            .optional()
            .ok()
            .flatten()
            .and_then(
                |(
                    scope_kind,
                    scope_key,
                    trust,
                    status_kind,
                    status_detail,
                    supersedes,
                    derived_from,
                    hash,
                    provenance,
                    ever_quarantined,
                )| {
                    let scope = scope_from_parts(&scope_kind, &scope_key, self.realm).ok()?;
                    let provenance: MemoryProvenance = serde_json::from_str(&provenance).ok()?;
                    Some(StagedRecordView {
                        scope,
                        trust: TrustTier::parse(&trust)?,
                        status: status_from_parts(&status_kind, status_detail),
                        supersedes,
                        derived_from: serde_json::from_str(&derived_from).unwrap_or_default(),
                        content_hash: hash,
                        has_verification: provenance.verification.is_some(),
                        ever_quarantined,
                    })
                },
            )
    }

    fn tombstoned_at_ms(&self, scope: &MemoryScope, hash: &str) -> Option<u64> {
        self.conn
            .query_row(
                "SELECT MAX(tombstoned_at_ms) FROM records WHERE scope_kind = ?1 \
                 AND scope_key = ?2 AND content_hash = ?3 AND status_kind = 'tombstoned'",
                params![scope.kind_str(), scope.key(), hash],
                |row| row.get::<_, Option<i64>>(0),
            )
            .ok()
            .flatten()
            .map(|ms| ms as u64)
    }
}

// ---- row mapping ----

struct MemoryRecordRow {
    memory_id: String,
    scope_kind: String,
    scope_key: String,
    kind: String,
    title: String,
    description: String,
    body: String,
    tags: String,
    provenance: String,
    trust: String,
    status_kind: String,
    status_detail: Option<String>,
    supersedes: Option<String>,
    derived_from: String,
    working_set_rank: Option<i64>,
    created_at_ms: i64,
    updated_at_ms: i64,
    usage_stats: String,
}

fn row_to_record_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecordRow> {
    Ok(MemoryRecordRow {
        memory_id: row.get(0)?,
        scope_kind: row.get(1)?,
        scope_key: row.get(2)?,
        kind: row.get(3)?,
        title: row.get(4)?,
        description: row.get(5)?,
        body: row.get(6)?,
        tags: row.get(7)?,
        provenance: row.get(8)?,
        trust: row.get(9)?,
        status_kind: row.get(10)?,
        status_detail: row.get(11)?,
        supersedes: row.get(12)?,
        derived_from: row.get(13)?,
        working_set_rank: row.get(14)?,
        created_at_ms: row.get(17)?,
        updated_at_ms: row.get(18)?,
        usage_stats: row.get(19)?,
    })
}

impl MemoryRecordRow {
    fn into_record(self, realm: &str) -> Result<super::records::MemoryRecord, AgentMemoryError> {
        let scope = scope_from_parts(&self.scope_kind, &self.scope_key, realm)?;
        let provenance: MemoryProvenance = serde_json::from_str(&self.provenance)
            .map_err(|err| AgentMemoryError::Parse(err.to_string()))?;
        Ok(super::records::MemoryRecord {
            id: self.memory_id,
            scope,
            kind: MemoryKind::parse(&self.kind).ok_or_else(|| {
                AgentMemoryError::Parse(format!("unknown record kind '{}'", self.kind))
            })?,
            title: self.title,
            description: self.description,
            body: self.body,
            tags: serde_json::from_str(&self.tags).unwrap_or_default(),
            provenance,
            trust: TrustTier::parse(&self.trust).ok_or_else(|| {
                AgentMemoryError::Parse(format!("unknown trust tier '{}'", self.trust))
            })?,
            status: status_from_parts(&self.status_kind, self.status_detail),
            supersedes: self.supersedes,
            derived_from: serde_json::from_str(&self.derived_from).unwrap_or_default(),
            working_set_rank: self.working_set_rank.map(|rank| rank as u32),
            created_at_ms: self.created_at_ms as u64,
            updated_at_ms: self.updated_at_ms as u64,
            usage: serde_json::from_str(&self.usage_stats).unwrap_or_default(),
        })
    }
}

fn status_from_parts(kind: &str, detail: Option<String>) -> RecordStatus {
    match kind {
        "superseded" => RecordStatus::Superseded {
            by: detail.unwrap_or_default(),
        },
        "quarantined" => RecordStatus::Quarantined {
            reason: detail.unwrap_or_default(),
        },
        "tombstoned" => RecordStatus::Tombstoned,
        _ => RecordStatus::Active,
    }
}

fn scope_from_parts(kind: &str, key: &str, realm: &str) -> Result<MemoryScope, AgentMemoryError> {
    match kind {
        "identity" => Ok(MemoryScope::Identity {
            realm: realm.to_string(),
            identity: key.to_string(),
        }),
        "mob" => Ok(MemoryScope::Mob {
            realm: realm.to_string(),
            mob: key.to_string(),
        }),
        "operator" => Ok(MemoryScope::Operator {
            realm: realm.to_string(),
            operator: key.to_string(),
        }),
        "realm" => Ok(MemoryScope::Realm {
            realm: realm.to_string(),
        }),
        other => Err(AgentMemoryError::Parse(format!(
            "unknown scope kind '{other}'"
        ))),
    }
}

fn active_scope_records(
    conn: &Connection,
    scope: &MemoryScope,
) -> Result<Vec<super::records::MemoryRecord>, AgentMemoryError> {
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {RECORD_COLUMNS} FROM records WHERE scope_kind = ?1 AND scope_key = ?2 \
             AND status_kind = 'active'"
        ))
        .map_err(sql_err)?;
    let rows = stmt
        .query_map(params![scope.kind_str(), scope.key()], row_to_record_row)
        .map_err(sql_err)?;
    let mut records = Vec::new();
    for row in rows {
        records.push(row.map_err(sql_err)?.into_record(scope.realm())?);
    }
    Ok(records)
}

fn load_record(
    conn: &Connection,
    realm: &str,
    memory_id: &str,
) -> Result<Option<super::records::MemoryRecord>, AgentMemoryError> {
    let row = conn
        .query_row(
            &format!("SELECT {RECORD_COLUMNS} FROM records WHERE memory_id = ?1"),
            params![memory_id],
            row_to_record_row,
        )
        .optional()
        .map_err(sql_err)?;
    row.map(|row| row.into_record(realm)).transpose()
}

/// Wire-compat projection: MemoryRecord → AgentMemoryRecord keeps
/// memory_id/title/body/tags/timestamps (§7.3 — recall stays
/// wire-compatible).
fn project_record(record: super::records::MemoryRecord) -> AgentMemoryRecord {
    AgentMemoryRecord {
        memory_id: record.id,
        title: record.title,
        body: record.body,
        tags: record.tags,
        created_at_ms: record.created_at_ms,
        updated_at_ms: record.updated_at_ms,
    }
}

/// §8.3 WorkingSet(k): top-K ranked (steward ordering) ∪ recent/unranked
/// slice (unranked, or updated since their last rank), newest first, the
/// union capped at 2*k. Full: every active record, ranked first.
fn scope_manifest(
    conn: &Connection,
    scope: &MemoryScope,
    tier: ManifestTier,
    now: u64,
) -> Result<Vec<RecordMeta>, AgentMemoryError> {
    let to_meta = |row: &rusqlite::Row<'_>| -> rusqlite::Result<RecordMeta> {
        let kind: String = row.get(1)?;
        let updated_at: i64 = row.get(4)?;
        let rank: Option<i64> = row.get(5)?;
        Ok(RecordMeta {
            id: row.get(0)?,
            kind: MemoryKind::parse(&kind).unwrap_or(MemoryKind::Fact),
            title: row.get(2)?,
            description: row.get(3)?,
            age_days: age_days(updated_at as u64, now),
            rank: rank.map(|rank| rank as u32),
        })
    };
    const META_COLUMNS: &str =
        "memory_id, kind, title, description, updated_at_ms, working_set_rank";
    match tier {
        ManifestTier::Full => {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {META_COLUMNS} FROM records \
                     WHERE scope_kind = ?1 AND scope_key = ?2 AND status_kind = 'active' \
                     ORDER BY (working_set_rank IS NULL) ASC, working_set_rank ASC, \
                     updated_at_ms DESC, created_at_ms DESC, rowid DESC"
                ))
                .map_err(sql_err)?;
            let rows = stmt
                .query_map(params![scope.kind_str(), scope.key()], to_meta)
                .map_err(sql_err)?;
            rows.collect::<Result<Vec<_>, _>>().map_err(sql_err)
        }
        ManifestTier::WorkingSet(k) => {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {META_COLUMNS} FROM records \
                     WHERE scope_kind = ?1 AND scope_key = ?2 AND status_kind = 'active' \
                     AND working_set_rank IS NOT NULL \
                     ORDER BY working_set_rank ASC, updated_at_ms DESC, rowid DESC LIMIT ?3"
                ))
                .map_err(sql_err)?;
            let ranked = stmt
                .query_map(params![scope.kind_str(), scope.key(), k as i64], to_meta)
                .map_err(sql_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_err)?;
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {META_COLUMNS} FROM records \
                     WHERE scope_kind = ?1 AND scope_key = ?2 AND status_kind = 'active' \
                     AND (working_set_rank IS NULL \
                          OR updated_at_ms > COALESCE(rank_set_at_ms, 0)) \
                     ORDER BY updated_at_ms DESC, created_at_ms DESC, rowid DESC LIMIT ?3"
                ))
                .map_err(sql_err)?;
            let recent = stmt
                .query_map(
                    params![scope.kind_str(), scope.key(), (2 * k) as i64],
                    to_meta,
                )
                .map_err(sql_err)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(sql_err)?;
            let cap = 2 * k;
            let mut seen = std::collections::HashSet::new();
            let mut union = Vec::new();
            for meta in ranked.into_iter().chain(recent) {
                if union.len() >= cap {
                    break;
                }
                if seen.insert(meta.id.clone()) {
                    union.push(meta);
                }
            }
            Ok(union)
        }
    }
}

/// §7.3 retention floors: warn (never evict) when a scope outgrows its
/// record-count or byte floor — retention pressure is a dream input, not a
/// FIFO.
fn warn_if_scope_floors_exceeded(
    conn: &Connection,
    scope: &MemoryScope,
    floor_records: usize,
    floor_bytes: usize,
) -> Result<(), AgentMemoryError> {
    let (count, bytes): (i64, Option<i64>) = conn
        .query_row(
            "SELECT COUNT(*), SUM(LENGTH(title) + LENGTH(description) + LENGTH(body)) \
             FROM records WHERE scope_kind = ?1 AND scope_key = ?2 \
             AND status_kind != 'tombstoned'",
            params![scope.kind_str(), scope.key()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(sql_err)?;
    if let Some(reason) = scope_floor_warning(
        count as usize,
        bytes.unwrap_or(0) as usize,
        floor_records,
        floor_bytes,
    ) {
        tracing::warn!(
            realm = scope.realm(),
            scope_kind = scope.kind_str(),
            scope_key = scope.key(),
            "agent memory scope exceeds retention floor ({reason}); steward consolidation \
             needed — records are never evicted automatically"
        );
    }
    Ok(())
}

/// Pure floor check, unit-tested separately from the tracing side effect.
fn scope_floor_warning(
    count: usize,
    bytes: usize,
    floor_records: usize,
    floor_bytes: usize,
) -> Option<String> {
    if count > floor_records {
        return Some(format!("{count} records > floor {floor_records}"));
    }
    if bytes > floor_bytes {
        return Some(format!("{bytes} bytes > floor {floor_bytes}"));
    }
    None
}

/// Markdown-import failure split: content problems are contained (skip the
/// file, keep the store open); I/O problems propagate into the open.
enum MarkdownImportError {
    Content(String),
    Io(AgentMemoryError),
}

/// One summary audit row per markdown-import file with skips or a wholesale
/// failure: the durable, operator-visible counterpart of the tracing warns.
fn record_import_audit(
    conn: &Connection,
    file: &Path,
    imported: usize,
    skipped: usize,
    reasons: &[String],
) -> Result<(), AgentMemoryError> {
    const MAX_AUDITED_REASONS: usize = 8;
    let detail = serde_json::json!({
        "op": "markdown_import",
        "file": file.display().to_string(),
        "imported": imported,
        "skipped": skipped,
        "skip_reasons": reasons.iter().take(MAX_AUDITED_REASONS).collect::<Vec<_>>(),
    });
    conn.execute(
        "INSERT INTO audit (stage_token, op_index, op_kind, memory_id, detail, applied_at_ms) \
         VALUES (?1, 0, 'import_summary', NULL, ?2, ?3)",
        params![
            mint_token("import-audit"),
            detail.to_string(),
            now_ms() as i64,
        ],
    )
    .map_err(sql_err)?;
    Ok(())
}

/// Add `column` to `table` when absent. Returns true when the column was
/// just added (the caller's cue to run a one-time backfill).
fn ensure_column(
    conn: &Connection,
    table: &str,
    column: &str,
    ddl: &str,
) -> Result<bool, AgentMemoryError> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(sql_err)?;
    let existing: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(sql_err)?
        .collect::<Result<_, _>>()
        .map_err(sql_err)?;
    if existing.iter().any(|name| name == column) {
        return Ok(false);
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {ddl}"),
        [],
    )
    .map_err(sql_err)?;
    Ok(true)
}

fn json_string<T: serde::Serialize>(value: &T) -> Result<String, AgentMemoryError> {
    serde_json::to_string(value).map_err(|err| AgentMemoryError::Parse(err.to_string()))
}

fn sql_err(err: rusqlite::Error) -> AgentMemoryError {
    AgentMemoryError::Io(err.to_string())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn mint_token(prefix: &str) -> String {
    static NEXT_TOKEN_SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = NEXT_TOKEN_SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!("{prefix}-{nanos}-{:x}-{seq:x}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity_first::agent_memory::{AgentMemorySelection, MarkdownAgentMemoryStore};
    use std::error::Error;

    fn identity() -> Result<AgentIdentity, Box<dyn Error>> {
        AgentIdentity::parse("identity:luka").map_err(|err| {
            std::io::Error::other(format!("test identity should parse: {err}")).into()
        })
    }

    fn identity_scope(realm: &str) -> Result<MemoryScope, Box<dyn Error>> {
        Ok(MemoryScope::Identity {
            realm: realm.to_string(),
            identity: identity()?.as_str().to_string(),
        })
    }

    fn new_memory(title: &str, body: &str) -> NewAgentMemory {
        NewAgentMemory {
            title: title.to_string(),
            body: body.to_string(),
            tags: Vec::new(),
        }
    }

    fn recall_all(identity: AgentIdentity, realm: &str) -> AgentMemoryRecallRequest {
        AgentMemoryRecallRequest {
            identity,
            realm: realm.to_string(),
            query_text: None,
            query_terms: Vec::new(),
            selection: AgentMemorySelection::Always,
            max_entries: 64,
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

    #[tokio::test]
    async fn remember_dedups_exact_content_hash() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;

        let first = store
            .remember("family", &id, new_memory("Same fact", "Same body"))
            .await?;
        let second = store
            .remember("family", &id, new_memory("Same fact", "Same body"))
            .await?;
        let third = store
            .remember("family", &id, new_memory("Other fact", "Other body"))
            .await?;

        assert_eq!(
            first.memory_id, second.memory_id,
            "dedup must return the existing id"
        );
        assert_ne!(first.memory_id, third.memory_id);
        let records = store.recall(recall_all(id, "family")).await?;
        assert_eq!(records.len(), 2, "duplicate remember must not add a row");
        Ok(())
    }

    #[tokio::test]
    async fn recall_scores_contextually_like_markdown_store() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;
        store
            .remember(
                "default",
                &id,
                NewAgentMemory {
                    title: "Passport location".to_string(),
                    body: "The passport is in the blue travel folder.".to_string(),
                    tags: vec!["travel".to_string()],
                },
            )
            .await?;
        store
            .remember(
                "default",
                &id,
                new_memory("Unrelated", "Rust release checklist."),
            )
            .await?;

        let matches = store
            .recall(AgentMemoryRecallRequest {
                identity: id,
                realm: "default".to_string(),
                query_text: Some("where did I put the passport".to_string()),
                query_terms: vec!["passport".to_string()],
                selection: AgentMemorySelection::Contextual,
                max_entries: 8,
            })
            .await?;

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].title, "Passport location");
        Ok(())
    }

    #[tokio::test]
    async fn forget_tombstones_and_allows_deliberate_readd() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;
        let record = store
            .remember("family", &id, new_memory("Fact", "Body"))
            .await?;

        let deleted = store.forget("family", &id, &record.memory_id).await?;
        assert!(deleted.deleted);
        assert!(
            store
                .recall(recall_all(id.clone(), "family"))
                .await?
                .is_empty()
        );

        let again = store.forget("family", &id, &record.memory_id).await?;
        assert!(!again.deleted, "tombstoned record must not delete twice");

        // A deliberate non-LLM re-add of the same content passes the
        // tombstone-recreation guard (which targets LLM authors, §8.4) and
        // mints a fresh id.
        let readded = store
            .remember("family", &id, new_memory("Fact", "Body"))
            .await?;
        assert_ne!(readded.memory_id, record.memory_id);
        Ok(())
    }

    #[tokio::test]
    async fn supersede_chains_and_inherits_rank() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;
        let scope = identity_scope("family")?;
        let prior = store
            .remember("family", &id, new_memory("DB host", "Use db-old.example."))
            .await?;

        // Steward ranks the record, then the RPC update path supersedes it.
        let token = store
            .stage(StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: "family".to_string(),
                author: MemoryAuthor::Steward {
                    run_id: "dream-1".to_string(),
                },
                ops: vec![StagedOp::SetRank {
                    id: prior.memory_id.clone(),
                    rank: Some(1),
                }],
            })
            .await?;
        store.commit(token).await?;

        let new_id = store
            .supersede(
                &scope,
                &prior.memory_id,
                payload("DB host", "Use db-new.example."),
            )
            .await?;
        assert_ne!(new_id, prior.memory_id);

        // Only the successor is recallable (memory never argues with
        // itself), and it inherited the steward rank.
        let records = store.recall(recall_all(id, "family")).await?;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].memory_id, new_id);
        assert!(records[0].body.contains("db-new"));

        let manifest = store.manifest(&[scope.clone()], ManifestTier::Full).await?;
        assert_eq!(manifest.len(), 1);
        assert_eq!(manifest[0].id, new_id);
        assert_eq!(
            manifest[0].rank,
            Some(1),
            "supersede inherits the prior's rank"
        );

        // Chain is preserved on the row.
        let conn = store.realm_connection("family")?;
        let guard = conn.lock().unwrap_or_else(|err| err.into_inner());
        let (status_kind, by): (String, Option<String>) = guard.query_row(
            "SELECT status_kind, status_detail FROM records WHERE memory_id = ?1",
            params![prior.memory_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(status_kind, "superseded");
        assert_eq!(by.as_deref(), Some(new_id.as_str()));
        Ok(())
    }

    #[tokio::test]
    async fn manifest_working_set_unions_ranked_and_recent() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;
        let scope = identity_scope("family")?;
        let mut ids = Vec::new();
        for i in 0..5 {
            let record = store
                .remember(
                    "family",
                    &id,
                    new_memory(&format!("Fact {i}"), &format!("Body {i}")),
                )
                .await?;
            ids.push(record.memory_id);
        }
        // Rank the first three; ranking does not count as an update, so the
        // ranked records leave the recent/unranked slice.
        let token = store
            .stage(StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: "family".to_string(),
                author: MemoryAuthor::Steward {
                    run_id: "dream-1".to_string(),
                },
                ops: (0..3)
                    .map(|i| StagedOp::SetRank {
                        id: ids[i].clone(),
                        rank: Some(i as u32 + 1),
                    })
                    .collect(),
            })
            .await?;
        store.commit(token).await?;

        let metas = store
            .manifest(&[scope.clone()], ManifestTier::WorkingSet(2))
            .await?;
        // top-2 ranked = ids[0], ids[1]; recent slice = the two unranked
        // (ids[4], ids[3] newest-first); union capped at 4.
        assert_eq!(metas.len(), 4);
        assert_eq!(metas[0].id, ids[0]);
        assert_eq!(metas[0].rank, Some(1));
        assert_eq!(metas[1].id, ids[1]);
        assert_eq!(metas[2].id, ids[4], "unranked slice is newest-first");
        assert_eq!(metas[3].id, ids[3]);
        assert!(
            !metas.iter().any(|meta| meta.id == ids[2]),
            "rank 3 is outside top-K and, being ranked and un-updated, outside the recent slice"
        );

        // A ranked record updated after its rank re-enters the recent slice
        // via supersede (rank inheritance keeps it selector-visible).
        let successor = store
            .supersede(&scope, &ids[2], payload("Fact 2", "Corrected body 2"))
            .await?;
        let metas = store
            .manifest(&[scope], ManifestTier::WorkingSet(2))
            .await?;
        assert!(
            metas.iter().any(|meta| meta.id == successor),
            "freshly superseded record must be selector-visible before the next dream: {metas:#?}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn staged_batch_without_commit_leaves_store_unchanged() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;
        let scope = identity_scope("family")?;

        let token = store
            .stage(StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: "family".to_string(),
                author: MemoryAuthor::Steward {
                    run_id: "dream-crash".to_string(),
                },
                ops: vec![StagedOp::Create {
                    id: None,
                    scope: scope.clone(),
                    record: payload("Staged fact", "Never committed"),
                    trust: TrustTier::AgentObserved,
                    derived_from: Vec::new(),
                    rationale: None,
                    created_at_ms: None,
                    updated_at_ms: None,
                }],
            })
            .await?;

        // The producer "dies": no commit. Nothing is visible, in this
        // instance or a fresh one over the same directory.
        assert!(
            store
                .recall(recall_all(id.clone(), "family"))
                .await?
                .is_empty()
        );
        let reopened = SqliteAgentMemoryStore::open(dir.path())?;
        assert!(
            reopened
                .recall(recall_all(id.clone(), "family"))
                .await?
                .is_empty()
        );

        // Commit applies the batch and burns the token.
        let receipt = store.commit(token.clone()).await?;
        assert_eq!(receipt.applied_ops, 1);
        assert_eq!(store.recall(recall_all(id, "family")).await?.len(), 1);
        let replay = store.commit(token).await;
        assert!(matches!(replay, Err(AgentMemoryError::InvalidRecord(_))));
        Ok(())
    }

    /// Stores created before the `ever_quarantined`/`taint` columns existed
    /// migrate on open, with the conservative backfill: currently-quarantined
    /// rows flagged directly, tombstoned rows flagged through their audit
    /// trail (the tombstone apply nulled the `quarantined` status_detail).
    #[tokio::test]
    async fn ever_quarantined_migration_backfills_old_stores() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let db_path = {
            let store = SqliteAgentMemoryStore::open(dir.path())?;
            store.path_for_realm("family")
        };
        {
            let conn = Connection::open(&db_path)?;
            conn.execute_batch(
                "CREATE TABLE records (
                    memory_id       TEXT PRIMARY KEY,
                    scope_kind      TEXT NOT NULL,
                    scope_key       TEXT NOT NULL,
                    kind            TEXT NOT NULL,
                    title           TEXT NOT NULL,
                    description     TEXT NOT NULL DEFAULT '',
                    body            TEXT NOT NULL,
                    tags            TEXT NOT NULL DEFAULT '[]',
                    provenance      TEXT NOT NULL,
                    trust           TEXT NOT NULL,
                    status_kind     TEXT NOT NULL,
                    status_detail   TEXT,
                    supersedes      TEXT,
                    derived_from    TEXT NOT NULL DEFAULT '[]',
                    working_set_rank INTEGER,
                    rank_set_at_ms  INTEGER,
                    content_hash    TEXT NOT NULL,
                    created_at_ms   INTEGER NOT NULL,
                    updated_at_ms   INTEGER NOT NULL,
                    usage_stats     TEXT NOT NULL DEFAULT '{}',
                    tombstoned_at_ms INTEGER
                );
                CREATE TABLE proposals (
                    proposal_id   TEXT PRIMARY KEY,
                    scope_kind    TEXT NOT NULL,
                    scope_key     TEXT NOT NULL,
                    record        TEXT NOT NULL,
                    author        TEXT NOT NULL,
                    status        TEXT NOT NULL DEFAULT 'pending',
                    created_at_ms INTEGER NOT NULL
                );
                CREATE TABLE audit (
                    audit_id      INTEGER PRIMARY KEY AUTOINCREMENT,
                    stage_token   TEXT NOT NULL,
                    op_index      INTEGER NOT NULL,
                    op_kind       TEXT NOT NULL,
                    memory_id     TEXT,
                    detail        TEXT NOT NULL,
                    applied_at_ms INTEGER NOT NULL
                );",
            )?;
            let provenance = "{\"author\":{\"author\":\"application\"}}";
            let insert = |id: &str, status_kind: &str, detail: Option<&str>| {
                conn.execute(
                    "INSERT INTO records (memory_id, scope_kind, scope_key, kind, title, \
                     description, body, tags, provenance, trust, status_kind, status_detail, \
                     supersedes, derived_from, content_hash, created_at_ms, updated_at_ms, \
                     usage_stats) VALUES (?1, 'identity', 'identity:luka', 'fact', ?1, '', \
                     'body', '[]', ?2, 'agent_observed', ?3, ?4, NULL, '[]', ?1, 1, 1, '{}')",
                    params![id, provenance, status_kind, detail],
                )
            };
            insert("mem-clean", "active", None)?;
            insert("mem-quarantined", "quarantined", Some("tainted session"))?;
            insert("mem-tombstoned-was-quarantined", "tombstoned", None)?;
            insert("mem-tombstoned-clean", "tombstoned", None)?;
            conn.execute(
                "INSERT INTO audit (stage_token, op_index, op_kind, memory_id, detail, \
                 applied_at_ms) VALUES ('direct-1', 0, 'create', \
                 'mem-tombstoned-was-quarantined', \
                 '{\"op\":\"create\",\"quarantined\":\"llm_writes=quarantined policy\"}', 1)",
                params![],
            )?;
        }
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        // Any realm operation opens the connection and runs the migration;
        // pending_proposals also exercises the proposals `taint` migration.
        assert!(store.pending_proposals("family", 4).await?.is_empty());
        let conn = store.realm_connection("family")?;
        let guard = conn.lock().unwrap_or_else(|err| err.into_inner());
        let flag = |id: &str| -> Result<bool, Box<dyn Error>> {
            Ok(guard.query_row(
                "SELECT ever_quarantined FROM records WHERE memory_id = ?1",
                params![id],
                |row| row.get(0),
            )?)
        };
        assert!(!flag("mem-clean")?);
        assert!(flag("mem-quarantined")?);
        assert!(flag("mem-tombstoned-was-quarantined")?);
        assert!(
            !flag("mem-tombstoned-clean")?,
            "ordinary tombstones must not be poisoned by the backfill"
        );
        Ok(())
    }

    /// The proposals `taint` migration conservatively marks still-live
    /// (pending/held) proposals tainted: the propose-time taint fact lived
    /// only in the in-memory tracker and is unrecoverable after the restart
    /// that accompanies the upgrade, so a plain steward accept downgrades
    /// to the operator gate instead of clean-accepting a possibly-tainted
    /// pre-migration proposal.
    #[tokio::test]
    async fn proposal_taint_migration_marks_live_proposals_tainted() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let db_path = {
            let store = SqliteAgentMemoryStore::open(dir.path())?;
            store.path_for_realm("family")
        };
        {
            let conn = Connection::open(&db_path)?;
            conn.execute_batch(
                "CREATE TABLE proposals (
                    proposal_id   TEXT PRIMARY KEY,
                    scope_kind    TEXT NOT NULL,
                    scope_key     TEXT NOT NULL,
                    record        TEXT NOT NULL,
                    author        TEXT NOT NULL,
                    status        TEXT NOT NULL DEFAULT 'pending',
                    created_at_ms INTEGER NOT NULL
                );",
            )?;
            let record = serde_json::to_string(&NewMemoryRecord {
                kind: MemoryKind::Fact,
                title: "Shared gotcha".to_string(),
                description: String::new(),
                body: "proposed before the taint column existed".to_string(),
                tags: Vec::new(),
                evidence: Vec::new(),
                verification: None,
            })?;
            let author = serde_json::to_string(&MemoryAuthor::Agent {
                identity: "identity:luka".to_string(),
            })?;
            let insert = |id: &str, status: &str| {
                conn.execute(
                    "INSERT INTO proposals (proposal_id, scope_kind, scope_key, record, \
                     author, status, created_at_ms) VALUES (?1, 'mob', 'mob:home', ?2, ?3, \
                     ?4, 1)",
                    params![id, record, author, status],
                )
            };
            insert("prop-pending", "pending")?;
            insert("prop-held", "held")?;
            insert("prop-accepted", "accepted")?;
            insert("prop-rejected", "rejected")?;
        }
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let proposals = store.pending_proposals("family", 8).await?;
        assert_eq!(proposals.len(), 2, "{proposals:?}");
        for proposal in &proposals {
            let taint = proposal.taint.as_deref().unwrap_or_else(|| {
                panic!(
                    "live pre-migration proposal '{}' must be conservatively tainted",
                    proposal.proposal_id
                )
            });
            assert!(taint.contains("pre-migration"), "{taint}");
        }
        // Terminal statuses are never re-verdicted: the backfill leaves them
        // alone.
        let conn = store.realm_connection("family")?;
        let guard = conn.lock().unwrap_or_else(|err| err.into_inner());
        for id in ["prop-accepted", "prop-rejected"] {
            let taint: Option<String> = guard.query_row(
                "SELECT taint FROM proposals WHERE proposal_id = ?1",
                params![id],
                |row| row.get(0),
            )?;
            assert!(
                taint.is_none(),
                "terminal proposal '{id}' must stay untouched: {taint:?}"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn stale_stage_tokens_gc_on_open() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let stage_create = |title: &str| StagedMutationBatch {
            kind: StagedBatchKind::FreshWrite,
            realm: "family".to_string(),
            author: MemoryAuthor::Application,
            ops: vec![StagedOp::Create {
                id: None,
                scope: identity_scope("family").expect("scope"),
                record: payload(title, &format!("{title} body")),
                trust: TrustTier::AgentObserved,
                derived_from: Vec::new(),
                rationale: None,
                created_at_ms: None,
                updated_at_ms: None,
            }],
        };
        let ungated = store.stage(stage_create("Stale")).await?;
        // A stage referenced by a still-PENDING gated promotion: the
        // operator's decision window outranks the dead-producer sweep.
        let pending_gated = store.stage(stage_create("Gated pending")).await?;
        store
            .record_pending_promotion(
                "family",
                PendingPromotion {
                    pending_id: "gate-pending".to_string(),
                    stage_token: pending_gated.token.clone(),
                    record_id: "mem-src-1".to_string(),
                    scope_kind: "mob".to_string(),
                    scope_key: "mob:home".to_string(),
                    rationale: None,
                    status: "pending".to_string(),
                    created_at_ms: now_ms(),
                },
            )
            .await?;
        // A stage referenced by a RESOLVED promotion must NOT be exempt —
        // this pins the `status = 'pending'` filter in the GC query.
        let resolved_gated = store.stage(stage_create("Gated resolved")).await?;
        store
            .record_pending_promotion(
                "family",
                PendingPromotion {
                    pending_id: "gate-resolved".to_string(),
                    stage_token: resolved_gated.token.clone(),
                    record_id: "mem-src-2".to_string(),
                    scope_kind: "mob".to_string(),
                    scope_key: "mob:home".to_string(),
                    rationale: None,
                    status: "pending".to_string(),
                    created_at_ms: now_ms(),
                },
            )
            .await?;
        store
            .resolve_pending_promotion("family", "gate-resolved", "denied")
            .await?;
        // Age every stage row past the 24h GC horizon, then reopen.
        {
            let conn = store.realm_connection("family")?;
            let guard = conn.lock().unwrap_or_else(|err| err.into_inner());
            guard.execute(
                "UPDATE stage SET created_at_ms = created_at_ms - ?1",
                params![(STAGE_GC_MAX_AGE_MS + 60_000) as i64],
            )?;
        }
        let reopened = SqliteAgentMemoryStore::open(dir.path())?;
        let result = reopened.commit(ungated).await;
        assert!(
            matches!(result, Err(AgentMemoryError::InvalidRecord(_))),
            "aged-out ungated stage token must be garbage-collected on open"
        );
        let result = reopened.commit(resolved_gated).await;
        assert!(
            matches!(result, Err(AgentMemoryError::InvalidRecord(_))),
            "a stage referenced only by a RESOLVED promotion must still be collected"
        );
        let receipt = reopened.commit(pending_gated).await.map_err(|err| {
            format!("a stage referenced by a pending gated promotion must survive GC: {err}")
        })?;
        assert_eq!(receipt.applied_ops, 1);
        Ok(())
    }

    #[tokio::test]
    async fn stage_rejects_lattice_violations() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let scope = identity_scope("family")?;

        // Agent author above the LLM ceiling.
        let above_ceiling = store
            .stage(StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: "family".to_string(),
                author: MemoryAuthor::Agent {
                    identity: identity()?.as_str().to_string(),
                },
                ops: vec![StagedOp::Create {
                    id: None,
                    scope: scope.clone(),
                    record: payload("Fact", "Body"),
                    trust: TrustTier::AgentVerified,
                    derived_from: Vec::new(),
                    rationale: None,
                    created_at_ms: None,
                    updated_at_ms: None,
                }],
            })
            .await;
        assert!(matches!(
            above_ceiling,
            Err(AgentMemoryError::InvalidRecord(_))
        ));

        // Operator tier is never staged-assignable, for any author.
        let operator_tier = store
            .stage(StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: "family".to_string(),
                author: MemoryAuthor::Operator,
                ops: vec![StagedOp::Create {
                    id: None,
                    scope,
                    record: payload("Fact", "Body"),
                    trust: TrustTier::Operator,
                    derived_from: Vec::new(),
                    rationale: None,
                    created_at_ms: None,
                    updated_at_ms: None,
                }],
            })
            .await;
        assert!(matches!(
            operator_tier,
            Err(AgentMemoryError::InvalidRecord(_))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn transitive_taint_blocks_laundering_through_store() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let scope = identity_scope("family")?;

        // Seed an untrusted record, merge it into a "fresh" consolidated
        // record, then try to retier the merge product upward.
        let seed = store
            .stage(StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: "family".to_string(),
                author: MemoryAuthor::Steward {
                    run_id: "dream-1".to_string(),
                },
                ops: vec![
                    StagedOp::Create {
                        id: Some("mem-tainted".to_string()),
                        scope: scope.clone(),
                        record: payload("Web claim", "Untrusted web content"),
                        trust: TrustTier::Untrusted,
                        derived_from: Vec::new(),
                        rationale: None,
                        created_at_ms: None,
                        updated_at_ms: None,
                    },
                    StagedOp::Create {
                        id: Some("mem-merged".to_string()),
                        scope: scope.clone(),
                        record: {
                            let mut merged = payload("Consolidated", "Merged content");
                            merged.verification = Some(super::super::records::VerificationClaim {
                                checked: "claims verification".to_string(),
                                evidence: Vec::new(),
                            });
                            merged
                        },
                        trust: TrustTier::AgentObserved,
                        derived_from: vec!["mem-tainted".to_string()],
                        rationale: Some("consolidation".to_string()),
                        created_at_ms: None,
                        updated_at_ms: None,
                    },
                ],
            })
            .await?;
        store.commit(seed).await?;

        let launder = store
            .stage(StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: "family".to_string(),
                author: MemoryAuthor::Steward {
                    run_id: "dream-2".to_string(),
                },
                ops: vec![StagedOp::Retier {
                    id: "mem-merged".to_string(),
                    trust: TrustTier::AgentVerified,
                    rationale: Some("launder attempt".to_string()),
                }],
            })
            .await;
        let err = match launder {
            Err(AgentMemoryError::InvalidRecord(message)) => message,
            other => return Err(format!("laundering must be rejected, got {other:?}").into()),
        };
        assert!(err.contains("untrusted/quarantined"), "{err}");
        Ok(())
    }

    #[tokio::test]
    async fn markdown_import_preserves_ids_and_renames_file() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let id = identity()?;
        let markdown = MarkdownAgentMemoryStore::open(dir.path())?;
        let first = markdown.remember(
            "family",
            &id,
            new_memory("Imported fact", "Body one with detail."),
        )?;
        let second = markdown.remember(
            "family",
            &id,
            NewAgentMemory {
                title: "Second fact".to_string(),
                body: "Body two with detail.".to_string(),
                tags: vec!["travel".to_string()],
            },
        )?;
        let md_path = markdown.path_for("family", &id);

        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let records = store.recall(recall_all(id.clone(), "family")).await?;
        let mut got: Vec<&str> = records.iter().map(|r| r.memory_id.as_str()).collect();
        got.sort_unstable();
        let mut want = [first.memory_id.as_str(), second.memory_id.as_str()];
        want.sort_unstable();
        assert_eq!(got, want, "import must preserve memory ids");
        let imported = records
            .iter()
            .find(|record| record.memory_id == second.memory_id)
            .ok_or("second record imported")?;
        assert_eq!(imported.tags, vec!["travel"]);
        assert_eq!(imported.created_at_ms, second.created_at_ms);

        assert!(
            !md_path.exists(),
            "markdown file must be renamed after import"
        );
        let renamed = md_path.with_extension("md.imported");
        assert!(renamed.exists(), "markdown file must survive as .imported");

        // Import audit trail exists (one audit row per imported record).
        let conn = store.realm_connection("family")?;
        let guard = conn.lock().unwrap_or_else(|err| err.into_inner());
        let audits: i64 = guard.query_row(
            "SELECT COUNT(*) FROM audit WHERE stage_token LIKE 'import-%'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(audits, 2);
        drop(guard);

        // Reopening does not re-import (file renamed) and keeps counts.
        let reopened = SqliteAgentMemoryStore::open(dir.path())?;
        assert_eq!(reopened.recall(recall_all(id, "family")).await?.len(), 2);
        Ok(())
    }

    /// Store-seam gate stand-in: quarantines LLM writes whose evidence
    /// cites the tainted session.
    struct TaintedSessionGate;

    impl crate::memory::taint::LlmWriteGate for TaintedSessionGate {
        fn quarantine_reason(
            &self,
            author: &MemoryAuthor,
            _kind: StagedBatchKind,
            evidence: &[crate::memory::records::EvidenceRef],
        ) -> Option<String> {
            if !author.is_llm() {
                return None;
            }
            evidence
                .iter()
                .any(|reference| reference.session_id == "tainted-sess")
                .then(|| "evidence cites a tainted session".to_string())
        }
    }

    fn tainted_evidence() -> Vec<crate::memory::records::EvidenceRef> {
        vec![crate::memory::records::EvidenceRef {
            session_id: "tainted-sess".to_string(),
            generation: 0,
            revision: None,
            range: None,
        }]
    }

    #[tokio::test]
    async fn release_then_retier_of_formerly_quarantined_origin_rejected()
    -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        store.set_llm_write_gate(std::sync::Arc::new(TaintedSessionGate));
        let scope = identity_scope("family")?;

        // Agent write from a tainted session lands quarantined, carrying a
        // verification claim (so the later retier passes the claim check
        // and only the taint ceiling can stop it).
        let mut record = payload("Quarantined origin", "possibly poisoned content");
        record.evidence = tainted_evidence();
        record.verification = Some(crate::memory::records::VerificationClaim {
            checked: "claims to have checked".to_string(),
            evidence: Vec::new(),
        });
        let receipt = store
            .remember_authored(
                &scope,
                record,
                MemoryAuthor::Agent {
                    identity: identity()?.as_str().to_string(),
                },
            )
            .await?;
        assert!(matches!(receipt.status, RecordStatus::Quarantined { .. }));
        let origin = receipt.memory_id;

        // Steward release: create a copy derived from the origin, tombstone
        // the origin (exactly the dream's release group).
        let mut copy_payload =
            payload("Quarantined origin", "possibly poisoned content (released)");
        copy_payload.verification = Some(crate::memory::records::VerificationClaim {
            checked: "claims to have checked".to_string(),
            evidence: Vec::new(),
        });
        let release = StagedMutationBatch {
            kind: StagedBatchKind::FreshWrite,
            realm: "family".to_string(),
            author: MemoryAuthor::Steward {
                run_id: "dream-1".to_string(),
            },
            ops: vec![
                StagedOp::Create {
                    id: Some("mem-released-copy".to_string()),
                    scope: scope.clone(),
                    record: copy_payload,
                    trust: TrustTier::AgentObserved,
                    derived_from: vec![origin.clone()],
                    rationale: Some("quarantine release".to_string()),
                    created_at_ms: None,
                    updated_at_ms: None,
                },
                StagedOp::Tombstone {
                    id: origin.clone(),
                    rationale: Some("superseded by quarantine release".to_string()),
                },
            ],
        };
        let token = store.stage(release).await?;
        store.commit(token).await?;
        let released = store
            .record_by_id("family", "mem-released-copy")
            .await?
            .ok_or("released copy exists")?;
        assert_eq!(released.status, RecordStatus::Active);
        let origin_record = store
            .record_by_id("family", &origin)
            .await?
            .ok_or("origin exists")?;
        assert_eq!(origin_record.status, RecordStatus::Tombstoned);

        // The durable taint marker persisted through the release: both the
        // tombstoned origin and the copy (inherited via derived_from).
        {
            let conn = store.realm_connection("family")?;
            let guard = conn.lock().unwrap_or_else(|err| err.into_inner());
            let flags: Vec<(String, bool)> = {
                let mut stmt = guard.prepare(
                    "SELECT memory_id, ever_quarantined FROM records ORDER BY memory_id",
                )?;
                let rows = stmt
                    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                rows
            };
            for (memory_id, flag) in &flags {
                assert!(
                    flag,
                    "'{memory_id}' must carry ever_quarantined after the release"
                );
            }
        }

        // §10.2 "capped forever": retiering the released copy to
        // agent_verified must be rejected even though the quarantined
        // origin is now tombstoned.
        let retier = StagedMutationBatch {
            kind: StagedBatchKind::FreshWrite,
            realm: "family".to_string(),
            author: MemoryAuthor::Steward {
                run_id: "dream-2".to_string(),
            },
            ops: vec![StagedOp::Retier {
                id: "mem-released-copy".to_string(),
                trust: TrustTier::AgentVerified,
                rationale: Some("post-release launder attempt".to_string()),
            }],
        };
        let err = store.stage(retier).await.expect_err("ceiling must hold");
        assert!(
            err.to_string().contains("provenance chain reaches"),
            "{err}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn markdown_import_skips_bad_records_and_files_loudly() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let id = identity()?;
        let markdown = MarkdownAgentMemoryStore::open(dir.path())?;
        let valid = markdown.remember("family", &id, new_memory("Valid fact", "Valid body."))?;
        let md_path = markdown.path_for("family", &id);

        // Hand-edits happen (§7.3 invites them): append one record with an
        // oversized title and one carrying a secret. Both must skip loudly;
        // the valid record must still import; the open must succeed.
        let oversized_title = "T".repeat(crate::memory::records::MAX_RECORD_TITLE_BYTES + 10);
        let mut content = fs::read_to_string(&md_path)?;
        content.push_str(&format!(
            "## {oversized_title}\n<!-- mobkit-agent-memory \
             {{\"memory_id\":\"mem-bad-title\",\"tags\":[],\"created_at_ms\":1,\
             \"updated_at_ms\":1}} -->\nSome body.\n<!-- /mobkit-agent-memory -->\n\n"
        ));
        content.push_str(
            "## Leaked credential\n<!-- mobkit-agent-memory \
             {\"memory_id\":\"mem-secret\",\"tags\":[],\"created_at_ms\":2,\
             \"updated_at_ms\":2} -->\nthe key was AKIAIOSFODNN7EXAMPLE\n\
             <!-- /mobkit-agent-memory -->\n\n",
        );
        fs::write(&md_path, content)?;

        // A file whose stem is not an agent identity (whitespace never
        // validates) fails wholesale: set aside as .import-failed, never
        // taking the realm store down.
        let junk_path = dir.path().join("family").join("not an identity.md");
        fs::write(&junk_path, "## Orphan\nnot a memory file\n")?;

        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let records = store.recall(recall_all(id.clone(), "family")).await?;
        assert_eq!(
            records
                .iter()
                .map(|record| record.memory_id.as_str())
                .collect::<Vec<_>>(),
            vec![valid.memory_id.as_str()],
            "only the valid record imports"
        );
        assert!(!md_path.exists(), "identity file renamed after import");
        assert!(md_path.with_extension("md.imported").exists());
        assert!(!junk_path.exists(), "junk file set aside");
        assert!(junk_path.with_extension("md.import-failed").exists());

        // The skips are counted in import audit rows.
        let conn = store.realm_connection("family")?;
        let guard = conn.lock().unwrap_or_else(|err| err.into_inner());
        let summaries: Vec<String> = {
            let mut stmt =
                guard.prepare("SELECT detail FROM audit WHERE op_kind = 'import_summary'")?;
            let rows = stmt
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<_>, _>>()?;
            rows
        };
        assert_eq!(summaries.len(), 2, "one summary per skipping/failing file");
        let identity_summary = summaries
            .iter()
            .find(|detail| detail.contains("mem-bad-title"))
            .ok_or("identity-file summary present")?;
        assert!(
            identity_summary.contains("\"skipped\":2"),
            "{identity_summary}"
        );
        assert!(
            identity_summary.contains("secret pattern class"),
            "{identity_summary}"
        );
        assert!(
            !identity_summary.contains("AKIAIOSFODNN7EXAMPLE"),
            "audit must not echo the secret: {identity_summary}"
        );
        drop(guard);

        // Reopen: no re-import attempts, store stays healthy.
        let reopened = SqliteAgentMemoryStore::open(dir.path())?;
        assert_eq!(reopened.recall(recall_all(id, "family")).await?.len(), 1);
        Ok(())
    }

    #[tokio::test]
    async fn propose_captures_taint_at_propose_time_and_refuses_secrets()
    -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        store.set_llm_write_gate(std::sync::Arc::new(TaintedSessionGate));
        let mob = MemoryScope::Mob {
            realm: "family".to_string(),
            mob: "mob:home".to_string(),
        };
        let author = MemoryAuthor::Agent {
            identity: identity()?.as_str().to_string(),
        };

        // Tainted at propose time: the fact is persisted on the row.
        let mut tainted = payload("Shared gotcha", "from a poisoned session");
        tainted.evidence = tainted_evidence();
        let tainted_id = store.propose(&mob, tainted, author.clone()).await?;
        // Clean propose: no taint.
        let clean_id = store
            .propose(
                &mob,
                payload("Clean gotcha", "from a clean session"),
                author.clone(),
            )
            .await?;
        let proposals = store.pending_proposals("family", 8).await?;
        let by_id: std::collections::HashMap<&str, &PendingProposal> = proposals
            .iter()
            .map(|proposal| (proposal.proposal_id.as_str(), proposal))
            .collect();
        let tainted_row = by_id.get(tainted_id.as_str()).ok_or("tainted present")?;
        assert!(
            tainted_row
                .taint
                .as_deref()
                .is_some_and(|reason| reason.contains("tainted")),
            "{:?}",
            tainted_row.taint
        );
        assert!(
            by_id
                .get(clean_id.as_str())
                .ok_or("clean present")?
                .taint
                .is_none()
        );

        // §10.4: the proposal seam refuses secrets with the class named.
        let err = store
            .propose(
                &mob,
                payload("Creds", "api_key = \"zXy1aB2cD3eF4gH5iJ6k\""),
                author,
            )
            .await
            .expect_err("secret-bearing proposal refused");
        let message = err.to_string();
        assert!(message.contains("credential-assignment"), "{message}");
        assert!(!message.contains("zXy1aB2cD3eF4gH5iJ6k"), "{message}");
        Ok(())
    }

    #[tokio::test]
    async fn secret_bearing_writes_refused_at_store_seam() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;
        // The wire remember path flows through the staged validator's
        // §10.4 chokepoint.
        let err = store
            .remember(
                "family",
                &id,
                new_memory("AWS key", "found AKIAIOSFODNN7EXAMPLE in the logs"),
            )
            .await
            .expect_err("secret-bearing remember refused");
        let message = err.to_string();
        assert!(message.contains("aws-access-key-id"), "{message}");
        assert!(!message.contains("AKIAIOSFODNN7EXAMPLE"), "{message}");

        // Clean writes pass.
        store
            .remember(
                "family",
                &id,
                new_memory(
                    "Key location",
                    "The AWS key lives in the vault, path infra/aws.",
                ),
            )
            .await?;
        Ok(())
    }

    #[tokio::test]
    async fn scope_floors_warn_but_never_evict() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?.with_scope_floors(2, usize::MAX);
        let id = identity()?;
        for i in 0..4 {
            store
                .remember(
                    "family",
                    &id,
                    new_memory(&format!("Fact {i}"), &format!("Body {i}")),
                )
                .await?;
        }
        let records = store.recall(recall_all(id, "family")).await?;
        assert_eq!(
            records.len(),
            4,
            "floors warn the steward; deterministic code never evicts"
        );
        Ok(())
    }

    #[test]
    fn floor_warning_fires_above_either_floor() {
        assert!(scope_floor_warning(5, 0, 4, 100).is_some());
        assert!(scope_floor_warning(0, 101, 4, 100).is_some());
        assert!(scope_floor_warning(4, 100, 4, 100).is_none());
    }

    #[tokio::test]
    async fn mark_usage_updates_counters() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;
        let record = store
            .remember("family", &id, new_memory("Fact", "Body"))
            .await?;

        store
            .mark_usage(&[record.memory_id.clone()], UsageEvent::Injected)
            .await?;
        store
            .mark_usage(&[record.memory_id.clone()], UsageEvent::ExplicitRecall)
            .await?;
        store
            .mark_usage(&[record.memory_id.clone()], UsageEvent::ExplicitRecall)
            .await?;
        store
            .mark_usage(&[record.memory_id.clone()], UsageEvent::JudgedUseful)
            .await?;

        let conn = store.realm_connection("family")?;
        let guard = conn.lock().unwrap_or_else(|err| err.into_inner());
        let usage_json: String = guard.query_row(
            "SELECT usage_stats FROM records WHERE memory_id = ?1",
            params![record.memory_id],
            |row| row.get(0),
        )?;
        let usage: UsageStats = serde_json::from_str(&usage_json)?;
        assert_eq!(usage.injected_count, 1, "ambient injections only");
        assert_eq!(usage.explicit_recall_count, 2, "explicit pulls only");
        assert_eq!(usage.judged_useful_count, 1);
        assert!(usage.last_injected_at_ms.is_some());
        assert!(usage.last_recalled_at_ms.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn injection_ledger_appends_and_reads_newest_first() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;
        let record = store
            .remember("family", &id, new_memory("Fact", "Body"))
            .await?;

        let build_entry = InjectionLogEntry {
            record_id: record.memory_id.clone(),
            identity: id.as_str().to_string(),
            session_key: None,
            surface: InjectionSurface::Build,
            at_ms: 100,
        };
        let turn_entry = InjectionLogEntry {
            record_id: record.memory_id.clone(),
            identity: id.as_str().to_string(),
            session_key: Some("session-1".to_string()),
            surface: InjectionSurface::Turn,
            at_ms: 200,
        };
        AgentMemoryProvider::log_injections(&store, "family", &[build_entry.clone()]).await?;
        AgentMemoryProvider::log_injections(&store, "family", &[turn_entry.clone()]).await?;

        let entries = store.injection_log("family", 16).await?;
        assert_eq!(entries, vec![turn_entry, build_entry]);

        let limited = store.injection_log("family", 1).await?;
        assert_eq!(limited.len(), 1);
        assert_eq!(limited[0].surface, InjectionSurface::Turn);

        let other_realm = store.injection_log("other", 16).await?;
        assert!(other_realm.is_empty(), "ledger rows are realm-scoped");
        Ok(())
    }

    #[tokio::test]
    async fn propose_queues_for_steward() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let scope = MemoryScope::Mob {
            realm: "family".to_string(),
            mob: "mob:home".to_string(),
        };
        let proposal_id = store
            .propose(
                &scope,
                payload("Shared fact", "For the mob store"),
                MemoryAuthor::Agent {
                    identity: identity()?.as_str().to_string(),
                },
            )
            .await?;
        assert!(proposal_id.starts_with("prop-"));

        let conn = store.realm_connection("family")?;
        let guard = conn.lock().unwrap_or_else(|err| err.into_inner());
        let (status, scope_kind): (String, String) = guard.query_row(
            "SELECT status, scope_kind FROM proposals WHERE proposal_id = ?1",
            params![proposal_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(status, "pending");
        assert_eq!(scope_kind, "mob");

        let author_json: String = guard.query_row(
            "SELECT author FROM proposals WHERE proposal_id = ?1",
            params![proposal_id],
            |row| row.get(0),
        )?;
        let author: MemoryAuthor = serde_json::from_str(&author_json)?;
        assert_eq!(
            author,
            MemoryAuthor::Agent {
                identity: identity()?.as_str().to_string()
            },
            "proposals carry real authorship (§8.2)"
        );
        Ok(())
    }

    // ---- §10.1 write gate ----

    /// Gate that quarantines every LLM-authored write (the
    /// `llm_writes = "quarantined"` posture / a permanently tainted session).
    struct AlwaysQuarantine;

    impl LlmWriteGate for AlwaysQuarantine {
        fn quarantine_reason(
            &self,
            author: &MemoryAuthor,
            _kind: StagedBatchKind,
            _evidence: &[crate::memory::records::EvidenceRef],
        ) -> Option<String> {
            author
                .is_llm()
                .then(|| "session tainted by web tool 'web_search'".to_string())
        }
    }

    fn agent_author() -> Result<MemoryAuthor, Box<dyn Error>> {
        Ok(MemoryAuthor::Agent {
            identity: identity()?.as_str().to_string(),
        })
    }

    #[tokio::test]
    async fn gated_agent_write_lands_quarantined_and_stays_unreadable() -> Result<(), Box<dyn Error>>
    {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        store.set_llm_write_gate(Arc::new(AlwaysQuarantine));
        let id = identity()?;
        let scope = identity_scope("family")?;

        let receipt = store
            .remember_authored(
                &scope,
                payload("Poisoned", "Attacker fact"),
                agent_author()?,
            )
            .await?;
        let RecordStatus::Quarantined { reason } = &receipt.status else {
            return Err(format!("expected quarantined status, got {:?}", receipt.status).into());
        };
        assert!(reason.contains("session tainted"), "{reason}");

        // Quarantined records are write-only: recall and manifest (the
        // coordinator's two read surfaces) must never return them.
        assert!(
            store
                .recall(recall_all(id.clone(), "family"))
                .await?
                .is_empty(),
            "quarantined bodies must never reach recall"
        );
        assert!(
            store
                .manifest(&[scope.clone()], ManifestTier::Full)
                .await?
                .is_empty(),
            "quarantined records must never reach the manifest"
        );

        // Non-LLM principals are not gated: the RPC remember path
        // (Application author) lands active through the same gate.
        let record = store
            .remember("family", &id, new_memory("App fact", "App body"))
            .await?;
        let records = store.recall(recall_all(id, "family")).await?;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].memory_id, record.memory_id);
        Ok(())
    }

    #[tokio::test]
    async fn distiller_write_law_holds_at_the_store_seam() -> Result<(), Box<dyn Error>> {
        use crate::memory::taint::{ContentTrustConfig, SessionTaintTracker, TaintLlmWriteGate};

        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let tracker = SessionTaintTracker::new(ContentTrustConfig::default());
        store.set_llm_write_gate(Arc::new(TaintLlmWriteGate::new(
            Some(tracker.clone()),
            crate::identity_first::agent_memory::AgentMemoryLlmWrites::Observed,
        )));
        let scope = identity_scope("family")?;
        let author = MemoryAuthor::Distiller {
            run_id: "run-1".to_string(),
        };
        let with_evidence = |title: &str, session: &str| NewMemoryRecord {
            evidence: vec![crate::memory::records::EvidenceRef {
                session_id: session.to_string(),
                generation: 1,
                revision: None,
                range: Some((0, 3)),
            }],
            ..payload(title, "Distilled body")
        };

        // Clean evidence: lands Active, tier-ceilinged at AgentObserved.
        let receipt = store
            .remember_authored(
                &scope,
                with_evidence("Clean fact", "sess-clean"),
                author.clone(),
            )
            .await?;
        assert_eq!(receipt.status, RecordStatus::Active);
        let record = store.with_realm_conn(&"family".to_string(), |conn| {
            load_record(conn, "family", &receipt.memory_id)?
                .ok_or_else(|| AgentMemoryError::Io("record missing".to_string()))
        })?;
        assert_eq!(record.trust, TrustTier::AgentObserved);
        assert!(matches!(
            record.provenance.author,
            MemoryAuthor::Distiller { .. }
        ));
        assert_eq!(record.provenance.evidence.len(), 1);
        assert_eq!(record.provenance.evidence[0].range, Some((0, 3)));

        // Tainted evidence range: session-tainted ⇒ the write quarantines,
        // for the Distiller author (not just Agent authors).
        tracker.note_current_session("identity:someone", "sess-dirty");
        tracker.observe_agent_event(
            "identity:someone",
            &meerkat_core::event::AgentEvent::ToolResultReceived {
                id: "t".to_string(),
                name: "web_fetch".to_string(),
                content: vec![],
                is_error: false,
            },
        );
        let receipt = store
            .remember_authored(
                &scope,
                with_evidence("Tainted fact", "sess-dirty"),
                author.clone(),
            )
            .await?;
        let RecordStatus::Quarantined { reason } = &receipt.status else {
            return Err(format!("expected quarantine, got {:?}", receipt.status).into());
        };
        assert!(reason.contains("evidence session tainted"), "{reason}");

        // Reset boundary: quarantines without any content taint (§8.4).
        tracker.mark_reset_boundary("sess-reset");
        let receipt = store
            .remember_authored(&scope, with_evidence("Reset fact", "sess-reset"), author)
            .await?;
        let RecordStatus::Quarantined { reason } = &receipt.status else {
            return Err(format!("expected quarantine, got {:?}", receipt.status).into());
        };
        assert!(reason.contains("reset boundary"), "{reason}");
        Ok(())
    }

    #[tokio::test]
    async fn recent_tombstones_lists_scope_tombstones_newest_first() -> Result<(), Box<dyn Error>> {
        use crate::memory::distiller::TombstoneSource;

        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;
        let scope = identity_scope("family")?;
        let kept = store
            .remember("family", &id, new_memory("Kept fact", "Body"))
            .await?;
        let dropped = store
            .remember("family", &id, new_memory("Phone number", "Body 2"))
            .await?;
        store.forget("family", &id, &dropped.memory_id).await?;

        let tombstones = store.recent_tombstones(&scope, 0, 10).await?;
        assert_eq!(tombstones.len(), 1);
        assert_eq!(tombstones[0].title, "Phone number");
        assert!(tombstones[0].tombstoned_at_ms > 0);
        // Active records never appear; a since_ms in the future filters out.
        assert!(!tombstones.iter().any(|t| t.title == "Kept fact"));
        let future = tombstones[0].tombstoned_at_ms + 1;
        assert!(
            store
                .recent_tombstones(&scope, future, 10)
                .await?
                .is_empty()
        );
        let _ = kept;
        Ok(())
    }

    #[tokio::test]
    async fn quarantined_supersede_leaves_prior_active() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;
        let scope = identity_scope("family")?;
        let prior = store
            .remember("family", &id, new_memory("DB host", "Use db-good.example."))
            .await?;

        store.set_llm_write_gate(Arc::new(AlwaysQuarantine));
        let receipt = store
            .supersede_authored(
                &scope,
                &prior.memory_id,
                payload("DB host", "Use db-evil.example."),
                agent_author()?,
            )
            .await?;
        assert!(matches!(receipt.status, RecordStatus::Quarantined { .. }));

        // A tainted "update" must not blank the good record.
        let records = store.recall(recall_all(id, "family")).await?;
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].memory_id, prior.memory_id);
        assert!(records[0].body.contains("db-good"));
        Ok(())
    }

    #[tokio::test]
    async fn gate_covers_staged_commits_not_just_direct_writes() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        store.set_llm_write_gate(Arc::new(AlwaysQuarantine));
        let id = identity()?;
        let scope = identity_scope("family")?;

        let token = store
            .stage(StagedMutationBatch {
                kind: StagedBatchKind::FreshWrite,
                realm: "family".to_string(),
                author: agent_author()?,
                ops: vec![StagedOp::Create {
                    id: None,
                    scope,
                    record: payload("Staged fact", "Via staged path"),
                    trust: TrustTier::AgentObserved,
                    derived_from: Vec::new(),
                    rationale: None,
                    created_at_ms: None,
                    updated_at_ms: None,
                }],
            })
            .await?;
        store.commit(token).await?;
        assert!(
            store.recall(recall_all(id, "family")).await?.is_empty(),
            "the write gate must hold at the store seam for staged commits too"
        );
        Ok(())
    }

    #[tokio::test]
    async fn ungated_authored_write_lands_active_with_agent_author() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;
        let scope = identity_scope("family")?;

        let mut record = payload("Observed fact", "Seen in session");
        record.verification = Some(super::super::records::VerificationClaim {
            checked: "ran the smoke test and watched it pass".to_string(),
            evidence: Vec::new(),
        });
        let receipt = store
            .remember_authored(&scope, record, agent_author()?)
            .await?;
        assert_eq!(receipt.status, RecordStatus::Active);

        // The verification is a CLAIM in provenance; the tier stays at the
        // LLM ceiling (§10.2).
        let conn = store.realm_connection("family")?;
        let guard = conn.lock().unwrap_or_else(|err| err.into_inner());
        let (trust, provenance_json): (String, String) = guard.query_row(
            "SELECT trust, provenance FROM records WHERE memory_id = ?1",
            params![receipt.memory_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        assert_eq!(trust, "agent_observed");
        let provenance: MemoryProvenance = serde_json::from_str(&provenance_json)?;
        assert_eq!(provenance.author, agent_author()?);
        assert!(
            provenance
                .verification
                .as_ref()
                .is_some_and(|claim| claim.checked.contains("smoke test"))
        );
        drop(guard);

        // Recall sees it (identity scope, active).
        let records = store.recall(recall_all(id, "family")).await?;
        assert_eq!(records.len(), 1);

        // forget_authored tombstones it with agent authorship.
        let scope = identity_scope("family")?;
        let result = store
            .forget_authored(&scope, &receipt.memory_id, agent_author()?)
            .await?;
        assert!(result.deleted);
        Ok(())
    }

    #[tokio::test]
    async fn authored_update_rejects_cross_identity_scope() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let id = identity()?;
        let prior = store
            .remember("family", &id, new_memory("Fact", "Body"))
            .await?;

        // An agent may only supersede within its OWN identity scope: the
        // staged validator rejects the batch even when the caller lies
        // about the scope (single-lineage supersede stays with the record's
        // own writers, §8.2).
        let other_scope = MemoryScope::Identity {
            realm: "family".to_string(),
            identity: "identity:other".to_string(),
        };
        let cross = store
            .supersede_authored(
                &other_scope,
                &prior.memory_id,
                payload("Fact", "Hijacked body"),
                MemoryAuthor::Agent {
                    identity: "identity:other".to_string(),
                },
            )
            .await;
        assert!(
            matches!(cross, Err(AgentMemoryError::InvalidRecord(_))),
            "cross-identity update must be rejected, got {cross:?}"
        );
        Ok(())
    }

    #[tokio::test]
    async fn panel_records_page_paginates_and_filters() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let scope = identity_scope("family")?;
        for index in 0..5 {
            store
                .remember_authored(
                    &scope,
                    payload(&format!("Fact {index}"), &format!("Body {index}")),
                    MemoryAuthor::Operator,
                )
                .await?;
        }

        // Keyset pagination: strictly-descending (updated_at_ms, id) with
        // no row repeated or skipped across pages.
        let first = store
            .records_page("family", Some("identity"), None, None, 2, None)
            .await?;
        assert_eq!(first.records.len(), 2);
        let cursor = first.next_cursor.clone().expect("more pages");
        let second = store
            .records_page("family", Some("identity"), None, None, 2, Some(cursor))
            .await?;
        assert_eq!(second.records.len(), 2);
        let third_cursor = second.next_cursor.clone().expect("one more page");
        let third = store
            .records_page(
                "family",
                Some("identity"),
                None,
                None,
                2,
                Some(third_cursor),
            )
            .await?;
        assert_eq!(third.records.len(), 1);
        assert_eq!(third.next_cursor, None);
        let mut seen: Vec<String> = first
            .records
            .iter()
            .chain(second.records.iter())
            .chain(third.records.iter())
            .map(|record| record.id.clone())
            .collect();
        let total = seen.len();
        seen.dedup();
        assert_eq!(total, 5, "pages cover every record exactly once");

        // Status filter.
        let quarantined = store
            .records_page("family", None, None, Some("quarantined"), 10, None)
            .await?;
        assert!(quarantined.records.is_empty());
        Ok(())
    }

    #[tokio::test]
    async fn panel_supersede_chain_walks_both_directions() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let scope = identity_scope("family")?;
        let root = store
            .remember_authored(&scope, payload("Fact", "v1"), MemoryAuthor::Operator)
            .await?;
        let mid = store
            .supersede_authored(
                &scope,
                &root.memory_id,
                payload("Fact", "v2"),
                MemoryAuthor::Operator,
            )
            .await?;
        let tip = store
            .supersede_authored(
                &scope,
                &mid.memory_id,
                payload("Fact", "v3"),
                MemoryAuthor::Operator,
            )
            .await?;

        // The same chain comes back oldest-first from every entry point.
        for entry in [&root.memory_id, &mid.memory_id, &tip.memory_id] {
            let chain = store.supersede_chain("family", entry, 16).await?;
            let ids: Vec<&str> = chain.iter().map(|record| record.id.as_str()).collect();
            assert_eq!(
                ids,
                [
                    root.memory_id.as_str(),
                    mid.memory_id.as_str(),
                    tip.memory_id.as_str()
                ],
                "chain from {entry}"
            );
        }
        // Bounded.
        let bounded = store.supersede_chain("family", &root.memory_id, 2).await?;
        assert_eq!(bounded.len(), 2);
        Ok(())
    }
}
