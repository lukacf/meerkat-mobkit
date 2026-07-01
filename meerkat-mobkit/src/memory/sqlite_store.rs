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
    AgentMemoryRecord, NewAgentMemory, compact_whitespace, decode_path_segment,
    encode_path_segment, new_memory_id, normalize_tags, read_markdown_records,
    select_recall_records,
};

use super::records::{
    ManifestTier, MemoryAuthor, MemoryId, MemoryKind, MemoryProvenance, MemoryScope,
    NewMemoryRecord, ProposalId, RecordMeta, RecordStatus, TrustTier, UsageEvent, UsageStats,
    age_days, content_hash, validate_record_fields,
};
use super::staged::{
    CommitReceipt, DEFAULT_TOMBSTONE_RECREATE_WINDOW_MS, StageToken, StagedBatchView,
    StagedMemoryStore, StagedMutationBatch, StagedOp, StagedRecordView, validate_batch,
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
    tombstoned_at_ms INTEGER
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
    created_at_ms INTEGER NOT NULL
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
        })
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
        let now = now_ms();
        conn.execute(
            "DELETE FROM stage WHERE created_at_ms < ?1",
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
            self.import_markdown_file(conn, realm, &path)?;
        }
        Ok(())
    }

    fn import_markdown_file(
        &self,
        conn: &mut Connection,
        realm: &str,
        path: &Path,
    ) -> Result<(), AgentMemoryError> {
        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            return Ok(());
        };
        let identity_str = decode_path_segment(stem);
        let identity = AgentIdentity::parse(&identity_str).map_err(|err| {
            AgentMemoryError::Parse(format!(
                "markdown import: '{}' does not decode to an agent identity: {err}",
                path.display()
            ))
        })?;
        let records = read_markdown_records(path)?;
        let scope = MemoryScope::Identity {
            realm: realm.to_string(),
            identity: identity.as_str().to_string(),
        };
        // Skip ids already present (idempotence if a rename previously
        // failed) and dedup ids within the file (hand-edits happen).
        let mut seen = std::collections::HashSet::new();
        let mut ops = Vec::new();
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
                .map_err(sql_err)?;
            if exists.is_some() {
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
        if !ops.is_empty() {
            let batch = StagedMutationBatch {
                realm: realm.to_string(),
                author: MemoryAuthor::Agent {
                    identity: identity.as_str().to_string(),
                },
                ops,
            };
            let token = mint_token("import");
            apply_batch_tx(conn, &batch, &token, now_ms()).map_err(|err| {
                AgentMemoryError::InvalidRecord(format!(
                    "markdown import of '{}' failed: {err}",
                    path.display()
                ))
            })?;
        }
        let mut imported_name = path.as_os_str().to_owned();
        imported_name.push(".imported");
        fs::rename(path, PathBuf::from(imported_name))
            .map_err(|err| AgentMemoryError::Io(err.to_string()))?;
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
            let receipt = apply_batch_tx(conn, &batch, &mint_token("direct"), now_ms())?;
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
        self.with_realm_conn(realm, |conn| {
            let record = load_record(conn, scope.realm(), &memory_id)?;
            let deletable = record.is_some_and(|record| {
                record.scope == scope && record.status != RecordStatus::Tombstoned
            });
            if !deletable {
                return Ok(AgentMemoryForgetResult {
                    memory_id,
                    deleted: false,
                });
            }
            let batch = StagedMutationBatch {
                realm: realm.to_string(),
                author: MemoryAuthor::Application,
                ops: vec![StagedOp::Tombstone {
                    id: memory_id.clone(),
                    rationale: None,
                }],
            };
            apply_batch_tx(conn, &batch, &mint_token("direct"), now_ms())?;
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
        let title = compact_whitespace(&record.title);
        let body = record.body.trim().to_string();
        validate_record_fields(&title, &record.description, &body)
            .map_err(AgentMemoryError::InvalidRecord)?;
        let tags = normalize_tags(record.tags)?;
        let realm = scope.realm().to_string();
        let expected_scope = scope.clone();
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
                realm: realm.clone(),
                author: MemoryAuthor::Application,
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
            let receipt = apply_batch_tx(conn, &batch, &mint_token("direct"), now_ms())?;
            receipt.memory_ids.first().cloned().ok_or_else(|| {
                AgentMemoryError::Io("supersede commit returned no record id".to_string())
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
                        // An explicit recall delivers the record into
                        // context — an injection by pull.
                        UsageEvent::Injected | UsageEvent::ExplicitRecall => {
                            usage.injected_count += 1;
                            usage.last_injected_at_ms = Some(now);
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

    fn propose_blocking(
        &self,
        scope: &MemoryScope,
        record: NewMemoryRecord,
    ) -> Result<ProposalId, AgentMemoryError> {
        validate_record_fields(&record.title, &record.description, &record.body)
            .map_err(AgentMemoryError::InvalidRecord)?;
        let proposal_id = mint_token("prop");
        self.with_realm_conn(scope.realm(), |conn| {
            conn.execute(
                "INSERT INTO proposals (proposal_id, scope_kind, scope_key, record, author, \
                 status, created_at_ms) VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6)",
                params![
                    proposal_id,
                    scope.kind_str(),
                    scope.key(),
                    json_string(&record)?,
                    json_string(&MemoryAuthor::Application)?,
                    now_ms() as i64,
                ],
            )
            .map_err(sql_err)?;
            Ok(())
        })?;
        Ok(proposal_id)
    }

    fn stage_blocking(&self, batch: StagedMutationBatch) -> Result<StageToken, AgentMemoryError> {
        let realm = batch.realm.clone();
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
            apply_batch_tx(conn, &batch, &token.token, now_ms())
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

    async fn propose(
        &self,
        scope: &MemoryScope,
        record: NewMemoryRecord,
    ) -> Result<ProposalId, AgentMemoryError> {
        let store = self.clone();
        let scope = scope.clone();
        run_blocking(move || store.propose_blocking(&scope, record)).await
    }

    fn supports_propose(&self) -> bool {
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

// ---- blocking internals ----

async fn run_blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, AgentMemoryError> + Send + 'static,
) -> Result<T, AgentMemoryError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| AgentMemoryError::Io(format!("agent memory task failed: {err}")))?
}

/// Validates (against the live transaction) and applies a batch atomically:
/// one SQLite transaction, one audit row per op (§8.5).
fn apply_batch_tx(
    conn: &mut Connection,
    batch: &StagedMutationBatch,
    token: &str,
    now: u64,
) -> Result<CommitReceipt, AgentMemoryError> {
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
        let memory_id = apply_op(&tx, batch, op, now)?;
        let detail = serde_json::json!({
            "op": op.kind_str(),
            "author": batch.author,
            "rationale": op_rationale(op),
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
                now,
                now,
            )?;
            conn.execute(
                "UPDATE records SET status_kind = 'superseded', status_detail = ?1, \
                 updated_at_ms = ?2 WHERE memory_id = ?3",
                params![memory_id, now as i64, prior],
            )
            .map_err(sql_err)?;
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
    conn.execute(
        "INSERT INTO records (memory_id, scope_kind, scope_key, kind, title, description, \
         body, tags, provenance, trust, status_kind, status_detail, supersedes, derived_from, \
         working_set_rank, rank_set_at_ms, content_hash, created_at_ms, updated_at_ms, \
         usage_stats, tombstoned_at_ms) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'active', NULL, ?11, ?12, ?13, \
         ?14, ?15, ?16, ?17, ?18, NULL)",
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
            supersedes,
            json_string(&derived_from.to_vec())?,
            working_set_rank,
            rank_set_at_ms,
            content_hash(&record.title, &record.body),
            created_at_ms as i64,
            updated_at_ms as i64,
            json_string(&UsageStats::default())?,
        ],
    )
    .map_err(sql_err)?;
    Ok(())
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
                 derived_from, content_hash, provenance FROM records WHERE memory_id = ?1",
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

    #[tokio::test]
    async fn stale_stage_tokens_gc_on_open() -> Result<(), Box<dyn Error>> {
        let dir = tempfile::tempdir()?;
        let store = SqliteAgentMemoryStore::open(dir.path())?;
        let token = store
            .stage(StagedMutationBatch {
                realm: "family".to_string(),
                author: MemoryAuthor::Application,
                ops: vec![StagedOp::Create {
                    id: None,
                    scope: identity_scope("family")?,
                    record: payload("Stale", "Stale body"),
                    trust: TrustTier::AgentObserved,
                    derived_from: Vec::new(),
                    rationale: None,
                    created_at_ms: None,
                    updated_at_ms: None,
                }],
            })
            .await?;
        // Age the row past the 24h GC horizon, then reopen.
        {
            let conn = store.realm_connection("family")?;
            let guard = conn.lock().unwrap_or_else(|err| err.into_inner());
            guard.execute(
                "UPDATE stage SET created_at_ms = created_at_ms - ?1",
                params![(STAGE_GC_MAX_AGE_MS + 60_000) as i64],
            )?;
        }
        let reopened = SqliteAgentMemoryStore::open(dir.path())?;
        let result = reopened.commit(token).await;
        assert!(
            matches!(result, Err(AgentMemoryError::InvalidRecord(_))),
            "aged-out stage token must be garbage-collected on open"
        );
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
        assert_eq!(usage.injected_count, 1);
        assert_eq!(usage.judged_useful_count, 1);
        assert!(usage.last_injected_at_ms.is_some());
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
            .propose(&scope, payload("Shared fact", "For the mob store"))
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
        Ok(())
    }
}
