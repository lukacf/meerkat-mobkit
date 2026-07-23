//! Read-only storage diagnosis for MobKit state directories (Phase M1 of the
//! storage unification arc).
//!
//! MobKit's side of the [`StorageMigrator`] diagnose seam: the same
//! shape-stable [`StorageDiagnosis`] report `rkat storage doctor` renders,
//! produced over a MobKit gateway state directory instead of a Meerkat realm
//! root. Exposed gateway-natively as the `mobkit/storage/doctor` RPC and a
//! console read method, because many MobKit deployments have no `rkat` CLI on
//! the box.
//!
//! # Safety contract — safe against a live gateway
//!
//! - never opens `Primary`-profile connections — only
//!   [`meerkat_sqlite::ConnectionProfile::ReadOnly`] opens and raw `SELECT`s;
//! - never creates files or directories;
//! - never runs the schema ledger (versions are read with
//!   [`meerkat_sqlite::domain_version`], nothing is applied);
//! - reads **only** the roots named in the [`DiagnoseScope`] — no ambient
//!   `$XDG_STATE_HOME`/`$HOME` resolution. An operator diagnosing the
//!   `mobkit_gateway` XDG home (peer key, runtime registry) passes it as a
//!   root explicitly.
//!
//! # Fault tolerance
//!
//! Per-entry: one unreadable database yields a finding for that file and
//! never aborts the sweep.
//!
//! # Scope semantics
//!
//! Each `state_roots` entry is one MobKit state directory (a gateway
//! `persistent_state` / `store_path` dir). MobKit state dirs are not
//! realm-keyed; `DiagnoseScope::realm` is honored as an *identity* filter on
//! the continuity checkpoint-evidence census and ignored elsewhere.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use meerkat_core::storage_diagnostics::{
    DatabaseInventory, DiagnoseScope, FindingSeverity, StorageDiagnosis, StorageDiagnosticsError,
    StorageFinding, StorageInventoryEntry, StorageMigrator,
};
use meerkat_core::{
    Session, SessionCheckpointMetadataState, SessionCheckpointState,
    session_metadata_document_from_slice,
};
use rusqlite::Connection;

use crate::auth::GATEWAY_PEER_KEY_FILE;
use crate::blob_store::is_valid_blob_id_value;
use crate::schedule_wiring::SCHEDULE_STORE_FILE;
use crate::storage_health::ResolvedStorageSummary;
use crate::workgraph_admission::WORKGRAPH_ADMISSION_SIDECAR_FILE;
use crate::workgraph_wiring::WORKGRAPH_STORE_FILE;

// Stable kebab-case finding codes (shape-stable: never renamed). Codes shared
// with `meerkat_store::doctor` keep its exact spelling so mixed reports
// aggregate cleanly.
/// Two (or more) spellings of the same database exist in one state directory
/// (`sessions.db` beside `sessions.sqlite`, `continuity.db` beside
/// `identity_continuity.sqlite`, `agent-memory/` beside
/// `agent-memory-sqlite/`, ...) — the MobKit split-brain census.
pub const FINDING_FILE_NAME_TWINS: &str = "file-name-twins";
/// An existing database file with no migration ledger (pre-M3; expected).
pub const FINDING_NO_SCHEMA_LEDGER: &str = "no-schema-ledger";
/// A database file that exists but contains no tables.
pub const FINDING_EMPTY_DATABASE_SHELL: &str = "empty-database-shell";
/// A database file that cannot be opened or queried read-only.
pub const FINDING_DATABASE_UNREADABLE: &str = "database-unreadable";
/// Continuity session snapshots without a typed checkpoint stamp (the census
/// H3's dry-run adoption consumes; finding is per identity).
pub const FINDING_LEGACY_UNVERIFIED_CONTINUITY_SNAPSHOTS: &str =
    "legacy-unverified-continuity-snapshots";
/// Snapshot checkpoint metadata that is present but malformed (never
/// laundered into "legacy").
pub const FINDING_CHECKPOINT_METADATA_INVALID: &str = "checkpoint-metadata-invalid";
/// A stamped continuity snapshot whose checkpoint digest does not verify
/// against its payload bytes (tampered or corrupted; restore rejects it).
/// Bounded work: digests are verified only for the stamped rows the census
/// already materializes — the payload bytes are in hand, only stamped rows
/// pay the full-session decode + digest.
pub const FINDING_CHECKPOINT_DIGEST_MISMATCH: &str = "checkpoint-digest-mismatch";
/// Continuity snapshot payloads that do not decode as a session document.
pub const FINDING_CONTINUITY_SNAPSHOT_UNDECODABLE: &str = "continuity-snapshot-undecodable";
/// A console frame references a blob object missing from the blob root.
pub const FINDING_DANGLING_CONSOLE_BLOB_REFERENCE: &str = "dangling-console-blob-reference";
/// Blob objects still stored in the legacy sharded FS layout
/// (`<blobs>/<first-2-hex>/<key>.json`), readable only through the legacy
/// fallback until a migration moves them.
pub const FINDING_LEGACY_FS_BLOBS: &str = "legacy-fs-blobs";
/// The blob root directory (inventory-grade, with object counts).
pub const FINDING_BLOB_ROOT: &str = "blob-root";
/// The gateway peer-key file (`peer_key.ed25519`) is present in this root.
pub const FINDING_PEER_KEY_FILE: &str = "peer-key-file";
/// The gateway runtime registry (`tux-runtimes.json`) is present in this
/// root.
pub const FINDING_RUNTIME_REGISTRY: &str = "runtime-registry";
/// The workgraph admission sidecar lock database is present.
pub const FINDING_WORKGRAPH_ADMISSION_SIDECAR: &str = "workgraph-admission-sidecar";
/// A `*.mfence` maintenance-fence lock file.
pub const FINDING_MAINTENANCE_FENCE_LOCK: &str = "maintenance-fence-lock";
/// A `*.pre-<version>-<timestamp>` migration backup artifact.
pub const FINDING_BACKUP_ARTIFACT: &str = "backup-artifact";
/// A `*.corrupt-<timestamp>` quarantined corrupt file.
pub const FINDING_QUARANTINE_ARTIFACT: &str = "quarantine-artifact";
/// H1 durability resolution of the blob slot (live runtime only).
pub const FINDING_BLOB_DURABILITY: &str = "blob-durability";
/// H2 incremental-persistence capability of the session store (live runtime
/// only).
pub const FINDING_SESSION_STORE_INCREMENTAL: &str = "session-store-incremental";
/// Durability-resolution census requires a live runtime handle; a cold
/// directory cannot see composition-time resolution.
pub const FINDING_DURABILITY_CENSUS_UNAVAILABLE: &str = "durability-census-unavailable";
/// A scoped state root does not exist (explicit roots make a typo worth
/// telling the operator about, unlike Meerkat's ambient candidate roots).
pub const FINDING_STATE_ROOT_MISSING: &str = "state-root-missing";
/// Internal doctor failure (the sweep task itself failed).
pub const FINDING_DOCTOR_INTERNAL: &str = "doctor-internal";

/// Cap on individually reported dangling console-frame blob references per
/// database; the remainder is summarized in one finding.
const DANGLING_BLOB_REPORT_CAP: usize = 50;

/// The gateway runtime-registry file name (`mobkit_gateway` XDG home).
/// Duplicated here because the deriving code lives in the gateway binary,
/// not the library.
const RUNTIME_REGISTRY_FILE: &str = "tux-runtimes.json";

/// One database domain with every filename spelling the three surfaces ever
/// used for it (the recon census), plus the ledger domains its owning stores
/// stamp. Two or more spellings existing side by side is the twin finding.
/// Shared with the M6 migrate verb, which reads the same inventory for its
/// ledger matrix.
pub(crate) struct DatabaseFamily {
    pub(crate) name: &'static str,
    pub(crate) spellings: &'static [&'static str],
    pub(crate) ledger_domains: &'static [&'static str],
}

/// The nine top-level databases across builder / `mobkit_gateway` /
/// `rpc_gateway` spellings, plus the M2 canonical `*.sqlite3` spellings so a
/// post-rename directory diagnosed with this binary censuses cleanly.
pub(crate) const DATABASE_FAMILIES: &[DatabaseFamily] = &[
    DatabaseFamily {
        name: "sessions",
        spellings: &["sessions.db", "sessions.sqlite", "sessions.sqlite3"],
        ledger_domains: &["session-store"],
    },
    DatabaseFamily {
        name: "runtime",
        spellings: &["runtime.sqlite"],
        ledger_domains: &["runtime-store"],
    },
    DatabaseFamily {
        name: "schedule",
        spellings: &[SCHEDULE_STORE_FILE],
        ledger_domains: &["schedule-store"],
    },
    DatabaseFamily {
        name: "workgraph",
        spellings: &[WORKGRAPH_STORE_FILE],
        ledger_domains: &["workgraph"],
    },
    DatabaseFamily {
        name: "workgraph-admission",
        spellings: &[WORKGRAPH_ADMISSION_SIDECAR_FILE],
        ledger_domains: &["mobkit-workgraph-admission"],
    },
    DatabaseFamily {
        name: "continuity",
        spellings: &[
            "continuity.db",
            "identity_continuity.sqlite",
            "continuity.sqlite3",
        ],
        ledger_domains: &["mobkit-continuity"],
    },
    DatabaseFamily {
        name: "metadata",
        spellings: &["mobkit_metadata.sqlite", "mobkit_metadata.sqlite3"],
        ledger_domains: &["mobkit-metadata"],
    },
    DatabaseFamily {
        name: "console",
        spellings: &["mobkit_console.sqlite", "mobkit_console.sqlite3"],
        ledger_domains: &["mobkit-console"],
    },
];

/// Agent-memory realm-root spellings: the builder's sqlite stack uses
/// `agent-memory-sqlite/` while `rpc_gateway` uses `agent-memory/` for the
/// same store kind — the third twin pair (a deployment moved between the two
/// surfaces silently orphans its whole memory realm corpus).
pub(crate) const MEMORY_ROOT_SPELLINGS: &[&str] = &["agent-memory", "agent-memory-sqlite"];

/// Ledger domain the per-realm memory databases stamp.
pub(crate) const MEMORY_LEDGER_DOMAIN: &str = "mobkit-memory";

/// Read-only diagnosis of MobKit state directories, cold (no live runtime;
/// the durability-resolution census reports
/// [`FINDING_DURABILITY_CENSUS_UNAVAILABLE`]). See the module docs for the
/// safety contract.
pub async fn diagnose_state_dir(scope: &DiagnoseScope) -> StorageDiagnosis {
    diagnose_state_dir_with_runtime(scope, None).await
}

/// Read-only diagnosis with the live runtime's composition-time storage
/// resolution attached as findings (H1/H2 durability census). Gateway RPC
/// surfaces pass [`crate::unified_runtime::UnifiedRuntime::resolved_storage`].
pub async fn diagnose_state_dir_with_runtime(
    scope: &DiagnoseScope,
    resolved: Option<ResolvedStorageSummary>,
) -> StorageDiagnosis {
    let scope = scope.clone();
    match tokio::task::spawn_blocking(move || diagnose_state_dir_blocking(&scope, resolved)).await {
        Ok(diagnosis) => diagnosis,
        Err(join_error) => {
            let mut diagnosis = StorageDiagnosis::default();
            diagnosis.findings.push(StorageFinding::new(
                FindingSeverity::Error,
                FINDING_DOCTOR_INTERNAL,
                format!("diagnosis sweep task failed: {join_error}"),
            ));
            diagnosis
        }
    }
}

/// Blocking diagnosis, for callers without an async context (the module-only
/// RPC path is synchronous).
pub fn diagnose_state_dir_blocking(
    scope: &DiagnoseScope,
    resolved: Option<ResolvedStorageSummary>,
) -> StorageDiagnosis {
    let mut diagnosis = StorageDiagnosis::default();

    // Dedup candidate roots by canonical identity while preserving order, so
    // two spellings of one directory do not double-report twins.
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut seen_roots: Vec<PathBuf> = Vec::new();
    for root in &scope.state_roots {
        let canonical = std::fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        if seen_roots.contains(&canonical) {
            continue;
        }
        seen_roots.push(canonical);
        roots.push(root.clone());
    }

    for root in &roots {
        sweep_state_dir(root, scope.realm.as_deref(), &mut diagnosis);
    }

    match resolved {
        Some(summary) => attach_live_durability(&mut diagnosis, summary),
        None => diagnosis.findings.push(StorageFinding::new(
            FindingSeverity::Info,
            FINDING_DURABILITY_CENSUS_UNAVAILABLE,
            "durability-resolution census unavailable: cold-directory diagnosis cannot see \
             composition-time resolution; invoke through a live gateway for the H1/H2 census",
        )),
    }

    diagnosis
}

/// The MobKit implementation of the [`StorageMigrator`] diagnose seam.
///
/// Deliberately a dumb unit struct delegating to [`diagnose_state_dir`], the
/// same shape as `meerkat_store::doctor::DiskStorageMigrator`.
///
/// The M6 mutation verbs ([`Self::migrate`], [`Self::prune`]) are **inherent
/// methods on this concrete type**, not additions to the meerkat-owned
/// [`StorageMigrator`] trait and not a mobkit-side extension trait: the core
/// trait stays diagnose-only (meerkat owns its contract), and an extension
/// trait would be uncallable through the `&dyn StorageMigrator` that
/// [`crate::storage_provider::MobKitStorageProvider::migrator`] returns
/// anyway. Callers that want mutation hold the concrete migrator — it is a
/// `Copy` unit struct, so "obtaining" one is `MobKitStorageMigrator` — and
/// the `mobkit_gateway storage-migrate` / `storage-prune` verbs are thin
/// wrappers over the same [`crate::storage_migrate`] library functions.
#[derive(Debug, Clone, Copy, Default)]
pub struct MobKitStorageMigrator;

impl MobKitStorageMigrator {
    /// The fenced M6 migration pass over one MobKit state directory
    /// (blocking; see [`crate::storage_migrate::migrate_state_dir`]). Async
    /// callers should wrap it in `spawn_blocking`.
    pub fn migrate(
        &self,
        state_dir: &std::path::Path,
        mode: crate::storage_migrate::MigrateMode,
        adopt: Option<&std::path::Path>,
    ) -> crate::storage_migrate::MobKitMigrateReport {
        crate::storage_migrate::migrate_state_dir(state_dir, mode, adopt)
    }

    /// Registered maintenance-artifact lifecycle over one MobKit state
    /// directory (blocking; see [`crate::storage_migrate::prune_state_dir`]).
    pub fn prune(
        &self,
        state_dir: &std::path::Path,
        older_than_days: u64,
        mode: crate::storage_migrate::MigrateMode,
    ) -> crate::storage_migrate::MobKitPruneReport {
        crate::storage_migrate::prune_state_dir(state_dir, older_than_days, mode)
    }
}

#[async_trait]
impl StorageMigrator for MobKitStorageMigrator {
    async fn diagnose(
        &self,
        scope: &DiagnoseScope,
    ) -> Result<StorageDiagnosis, StorageDiagnosticsError> {
        Ok(diagnose_state_dir(scope).await)
    }
}

fn attach_live_durability(diagnosis: &mut StorageDiagnosis, summary: ResolvedStorageSummary) {
    diagnosis.findings.push(StorageFinding::new(
        FindingSeverity::Info,
        FINDING_BLOB_DURABILITY,
        format!(
            "blob slot resolved to '{}' (persistent: {})",
            summary.blob_durability.as_str(),
            summary.blob_durability.is_persistent()
        ),
    ));
    let message = match summary.session_store_incremental {
        Some(true) => "session store advertises incremental persistence".to_string(),
        Some(false) => "session store lacks incremental persistence; session persistence \
                        degrades to whole-blob saves on every turn"
            .to_string(),
        None => "no persistent session service (ephemeral session lifecycle)".to_string(),
    };
    diagnosis.findings.push(StorageFinding::new(
        FindingSeverity::Info,
        FINDING_SESSION_STORE_INCREMENTAL,
        message,
    ));
}

fn sweep_state_dir(state_dir: &Path, identity_filter: Option<&str>, out: &mut StorageDiagnosis) {
    if !state_dir.is_dir() {
        out.findings.push(
            StorageFinding::new(
                FindingSeverity::Info,
                FINDING_STATE_ROOT_MISSING,
                "scoped state directory does not exist",
            )
            .with_path(state_dir.to_path_buf()),
        );
        return;
    }

    let label = state_dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| state_dir.display().to_string());
    let mut entry = StorageInventoryEntry::new(label, state_dir.to_path_buf());

    // Top-level database census: inventory, empty shells, twins, ledger
    // state per family.
    for family in DATABASE_FAMILIES {
        let present: Vec<PathBuf> = family
            .spellings
            .iter()
            .map(|spelling| state_dir.join(spelling))
            .filter(|path| path.is_file())
            .collect();
        report_twins(family.name, &present, out);
        for db_path in &present {
            entry
                .databases
                .push(inspect_database(db_path, family.ledger_domains, out));
        }
        if family.name == "continuity" {
            for db_path in &present {
                census_continuity_snapshots(db_path, identity_filter, out);
            }
        }
        if family.name == "console" {
            for db_path in &present {
                sweep_console_blob_references(db_path, state_dir, out);
            }
        }
        if family.name == "workgraph-admission" && !present.is_empty() {
            out.findings.push(
                StorageFinding::new(
                    FindingSeverity::Info,
                    FINDING_WORKGRAPH_ADMISSION_SIDECAR,
                    "workgraph admission sidecar lock database (cross-process admission lock; \
                     the file persists after normal use — a held RESERVED lock means a live \
                     process is mid-admission)",
                )
                .with_path(present[0].clone()),
            );
        }
    }

    // Agent-memory realm roots: twin census across the two spellings, then
    // per-realm database inventory inside each existing root.
    let memory_roots: Vec<PathBuf> = MEMORY_ROOT_SPELLINGS
        .iter()
        .map(|spelling| state_dir.join(spelling))
        .filter(|path| path.is_dir())
        .collect();
    report_twins("agent-memory", &memory_roots, out);
    for memory_root in &memory_roots {
        sweep_memory_root(memory_root, &mut entry, out);
    }

    sweep_blob_root(state_dir, out);

    // Gateway-home artifacts, reported when materialized inside a scoped
    // root (the mobkit_gateway XDG home is itself a valid root to pass).
    let peer_key = state_dir.join(GATEWAY_PEER_KEY_FILE);
    if peer_key.is_file() {
        out.findings.push(
            StorageFinding::new(
                FindingSeverity::Info,
                FINDING_PEER_KEY_FILE,
                "gateway peer signing key",
            )
            .with_path(peer_key),
        );
    }
    let registry = state_dir.join(RUNTIME_REGISTRY_FILE);
    if registry.is_file() {
        out.findings.push(
            StorageFinding::new(
                FindingSeverity::Info,
                FINDING_RUNTIME_REGISTRY,
                "gateway runtime registry",
            )
            .with_path(registry),
        );
    }

    // Filesystem artifacts beside the databases (state root plus the memory
    // realm roots).
    let mut artifact_dirs = vec![state_dir.to_path_buf()];
    artifact_dirs.extend(memory_roots);
    for dir in &artifact_dirs {
        sweep_artifacts(dir, out);
    }

    out.inventory.push(entry);
}

/// Two or more spellings of one database family in the same directory is the
/// split-brain twin finding: every surface keeps writing its own spelling and
/// the histories silently diverge.
fn report_twins(family: &str, present: &[PathBuf], out: &mut StorageDiagnosis) {
    if present.len() < 2 {
        return;
    }
    let paths = present
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>()
        .join(" and ");
    out.findings.push(
        StorageFinding::new(
            FindingSeverity::Error,
            FINDING_FILE_NAME_TWINS,
            format!(
                "{} spellings of the '{family}' store exist side by side: {paths}; surfaces \
                 disagree on which file is authoritative — reconcile before writing through \
                 either copy (migration lands in Phase M6)",
                present.len()
            ),
        )
        .with_path(present[0].clone()),
    );
}

fn table_exists(conn: &Connection, table: &str) -> Result<bool, rusqlite::Error> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [table],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

fn user_table_count(conn: &Connection) -> Result<i64, rusqlite::Error> {
    conn.query_row(
        "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table'",
        [],
        |row| row.get(0),
    )
}

/// Inventory one database file: empty-shell probe plus schema-ledger state
/// for the expected domains (and any unexpected ledger rows).
fn inspect_database(
    db_path: &Path,
    expected_domains: &[&str],
    out: &mut StorageDiagnosis,
) -> DatabaseInventory {
    let mut inventory = DatabaseInventory::new(db_path.to_path_buf());
    let conn = match meerkat_sqlite::open(db_path, meerkat_sqlite::ConnectionProfile::ReadOnly) {
        Ok(conn) => conn,
        Err(err) => {
            out.findings.push(
                StorageFinding::new(
                    FindingSeverity::Error,
                    FINDING_DATABASE_UNREADABLE,
                    format!("cannot open database read-only: {err}"),
                )
                .with_path(db_path.to_path_buf()),
            );
            return inventory;
        }
    };

    match user_table_count(&conn) {
        Ok(0) => {
            out.findings.push(
                StorageFinding::new(
                    FindingSeverity::Info,
                    FINDING_EMPTY_DATABASE_SHELL,
                    "database file exists but contains no tables (empty shell)",
                )
                .with_path(db_path.to_path_buf()),
            );
        }
        Ok(_) => {}
        Err(err) => {
            out.findings.push(
                StorageFinding::new(
                    FindingSeverity::Error,
                    FINDING_DATABASE_UNREADABLE,
                    format!("cannot read sqlite_master: {err}"),
                )
                .with_path(db_path.to_path_buf()),
            );
            return inventory;
        }
    }

    let ledger_present = match table_exists(&conn, "meerkat_schema") {
        Ok(present) => present,
        Err(err) => {
            out.findings.push(
                StorageFinding::new(
                    FindingSeverity::Error,
                    FINDING_DATABASE_UNREADABLE,
                    format!("cannot read schema ledger: {err}"),
                )
                .with_path(db_path.to_path_buf()),
            );
            return inventory;
        }
    };

    if !ledger_present {
        out.findings.push(
            StorageFinding::new(
                FindingSeverity::Info,
                FINDING_NO_SCHEMA_LEDGER,
                "existing database has no meerkat_schema ledger (written before the M3 \
                 shared-mechanics port; expected — the owning store baselines it on next \
                 write open)",
            )
            .with_path(db_path.to_path_buf()),
        );
        for expected in expected_domains {
            inventory.domains.push(((*expected).to_string(), None));
        }
        return inventory;
    }

    for expected in expected_domains {
        match meerkat_sqlite::domain_version(&conn, expected) {
            Ok(version) => inventory.domains.push(((*expected).to_string(), version)),
            Err(err) => out.findings.push(
                StorageFinding::new(
                    FindingSeverity::Error,
                    FINDING_DATABASE_UNREADABLE,
                    format!("cannot read ledger version for domain '{expected}': {err}"),
                )
                .with_path(db_path.to_path_buf()),
            ),
        }
    }
    // Surface ledger rows for domains this census did not expect on the file
    // (versions reported without judgment; supported-version authority lives
    // with the owning stores).
    let extra_rows = (|| -> Result<Vec<(String, i64)>, rusqlite::Error> {
        let mut statement =
            conn.prepare("SELECT domain, version FROM meerkat_schema ORDER BY domain")?;
        let rows = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    })();
    match extra_rows {
        Ok(rows) => {
            for (domain, version) in rows {
                if !inventory.domains.iter().any(|(name, _)| *name == domain) {
                    inventory.domains.push((domain, Some(version)));
                }
            }
        }
        Err(err) => out.findings.push(
            StorageFinding::new(
                FindingSeverity::Error,
                FINDING_DATABASE_UNREADABLE,
                format!("cannot enumerate schema ledger rows: {err}"),
            )
            .with_path(db_path.to_path_buf()),
        ),
    }
    inventory
}

/// Checkpoint-evidence census over a continuity store: raw read-only SQL
/// over `session_snapshots.data`, each payload decoded as a Meerkat session
/// document and classified through the core metadata census helper. This is
/// the per-identity stamped/legacy count H3's dry-run adoption consumes.
///
/// A structural stamp is not counted as healthy on its own: restore verifies
/// the stamp digest against the full document, so the census does too — a
/// stamped row whose digest fails verification is an error-severity
/// [`FINDING_CHECKPOINT_DIGEST_MISMATCH`] naming the session. The payload
/// bytes are already loaded for the structural check; only stamped rows pay
/// the additional full-session decode + digest.
fn census_continuity_snapshots(
    db_path: &Path,
    identity_filter: Option<&str>,
    out: &mut StorageDiagnosis,
) {
    let Ok(conn) = meerkat_sqlite::open(db_path, meerkat_sqlite::ConnectionProfile::ReadOnly)
    else {
        return; // already reported by inspect_database
    };
    match table_exists(&conn, "session_snapshots") {
        Ok(true) => {}
        Ok(false) => return,
        Err(_) => return, // already reported by inspect_database
    }

    // identity -> (stamped-and-verified, legacy)
    let mut census: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    let mut invalid = 0usize;
    let mut undecodable = 0usize;
    // (session id, identity, verification error) per stamped row whose
    // digest fails against the payload bytes.
    let mut digest_failures: Vec<(String, String, String)> = Vec::new();

    let result = (|| -> Result<(), rusqlite::Error> {
        let mut statement =
            conn.prepare("SELECT session_id, identity, data FROM session_snapshots")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let session_id: String = row.get(0)?;
            let identity: String = row.get(1)?;
            if identity_filter.is_some_and(|filter| filter != identity) {
                continue;
            }
            let data: Vec<u8> = row.get(2)?;
            let Ok(document) = session_metadata_document_from_slice(&data) else {
                undecodable += 1;
                continue;
            };
            match document.try_checkpoint_metadata_state() {
                Ok(SessionCheckpointMetadataState::Stamped(_)) => {
                    match serde_json::from_slice::<Session>(&data) {
                        Ok(session) => match session.try_checkpoint_state() {
                            Ok(SessionCheckpointState::Verified(_)) => {
                                census.entry(identity).or_default().0 += 1;
                            }
                            Ok(SessionCheckpointState::LegacyUnverified { .. }) => {
                                census.entry(identity).or_default().1 += 1;
                            }
                            Err(error) => {
                                digest_failures.push((session_id, identity, error.to_string()));
                            }
                        },
                        Err(_) => undecodable += 1,
                    }
                }
                Ok(SessionCheckpointMetadataState::LegacyUnverified { .. }) => {
                    census.entry(identity).or_default().1 += 1;
                }
                Err(_) => invalid += 1,
            }
        }
        Ok(())
    })();

    if let Err(err) = result {
        out.findings.push(
            StorageFinding::new(
                FindingSeverity::Error,
                FINDING_DATABASE_UNREADABLE,
                format!("continuity snapshot census query failed: {err}"),
            )
            .with_path(db_path.to_path_buf()),
        );
        return;
    }

    for (identity, (stamped, legacy)) in &census {
        if *legacy > 0 {
            out.findings.push(
                StorageFinding::new(
                    FindingSeverity::Warning,
                    FINDING_LEGACY_UNVERIFIED_CONTINUITY_SNAPSHOTS,
                    format!(
                        "{legacy} legacy-unverified session snapshot(s) ({stamped} stamped) for \
                         identity '{identity}'; checkpoint adoption arrives with H3"
                    ),
                )
                .with_path(db_path.to_path_buf())
                .with_realm(identity.clone()),
            );
        }
    }
    for (session_id, identity, error) in &digest_failures {
        out.findings.push(
            StorageFinding::new(
                FindingSeverity::Error,
                FINDING_CHECKPOINT_DIGEST_MISMATCH,
                format!(
                    "stamped snapshot for session '{session_id}' fails checkpoint digest \
                     verification ({error}); restore will reject it"
                ),
            )
            .with_path(db_path.to_path_buf())
            .with_realm(identity.clone()),
        );
    }
    if invalid > 0 {
        out.findings.push(
            StorageFinding::new(
                FindingSeverity::Error,
                FINDING_CHECKPOINT_METADATA_INVALID,
                format!(
                    "{invalid} snapshot(s) carry malformed checkpoint metadata \
                     (present-but-invalid evidence is never laundered into legacy)"
                ),
            )
            .with_path(db_path.to_path_buf()),
        );
    }
    if undecodable > 0 {
        out.findings.push(
            StorageFinding::new(
                FindingSeverity::Warning,
                FINDING_CONTINUITY_SNAPSHOT_UNDECODABLE,
                format!("{undecodable} snapshot payload(s) did not decode as a session document"),
            )
            .with_path(db_path.to_path_buf()),
        );
    }
}

/// The on-disk object paths `ObjectStoreBlobStore` uses for a canonical blob
/// id: `objects/<key>.bin` (current) and `<first-2-hex>/<key>.json` (legacy
/// FS layout, still read through the fallback).
fn blob_object_exists(blobs_root: &Path, blob_id: &str) -> bool {
    if !is_valid_blob_id_value(blob_id) {
        return false;
    }
    let Some(key) = blob_id.strip_prefix("sha256:") else {
        return false;
    };
    if blobs_root
        .join("objects")
        .join(format!("{key}.bin"))
        .is_file()
    {
        return true;
    }
    let prefix = key.get(0..2).unwrap_or("xx");
    blobs_root
        .join(prefix)
        .join(format!("{key}.json"))
        .is_file()
}

/// Dangling console-frame → blob sweep: console frames carry blob references
/// in their JSON payload (`payload_json.blob_id`, e.g. `assistant_image`
/// frames); each referenced object is probed under the state dir's blob root.
fn sweep_console_blob_references(db_path: &Path, state_dir: &Path, out: &mut StorageDiagnosis) {
    let Ok(conn) = meerkat_sqlite::open(db_path, meerkat_sqlite::ConnectionProfile::ReadOnly)
    else {
        return; // already reported by inspect_database
    };
    match table_exists(&conn, "console_frames") {
        Ok(true) => {}
        Ok(false) => return,
        Err(_) => return,
    }

    let mut referenced: BTreeSet<String> = BTreeSet::new();
    let result = (|| -> Result<(), rusqlite::Error> {
        let mut statement = conn.prepare("SELECT payload_json FROM console_frames")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let payload_json: String = row.get(0)?;
            if let Ok(payload) = serde_json::from_str::<serde_json::Value>(&payload_json)
                && let Some(blob_id) = payload.get("blob_id").and_then(serde_json::Value::as_str)
            {
                referenced.insert(blob_id.to_string());
            }
        }
        Ok(())
    })();

    if let Err(err) = result {
        out.findings.push(
            StorageFinding::new(
                FindingSeverity::Error,
                FINDING_DATABASE_UNREADABLE,
                format!("console frame blob sweep query failed: {err}"),
            )
            .with_path(db_path.to_path_buf()),
        );
        return;
    }

    let blobs_root = state_dir.join("blobs");
    let dangling: Vec<&String> = referenced
        .iter()
        .filter(|blob_id| !blob_object_exists(&blobs_root, blob_id))
        .collect();
    let total = dangling.len();
    for blob_id in dangling.iter().take(DANGLING_BLOB_REPORT_CAP) {
        out.findings.push(
            StorageFinding::new(
                FindingSeverity::Error,
                FINDING_DANGLING_CONSOLE_BLOB_REFERENCE,
                format!("console frame references missing blob {blob_id}"),
            )
            .with_path(db_path.to_path_buf()),
        );
    }
    if total > DANGLING_BLOB_REPORT_CAP {
        out.findings.push(
            StorageFinding::new(
                FindingSeverity::Error,
                FINDING_DANGLING_CONSOLE_BLOB_REFERENCE,
                format!(
                    "{} additional dangling console blob reference(s) not listed individually \
                     ({total} total)",
                    total - DANGLING_BLOB_REPORT_CAP
                ),
            )
            .with_path(db_path.to_path_buf()),
        );
    }
}

/// Blob-root inventory: current-layout object count plus the legacy sharded
/// FS layout census (`<blobs>/<first-2-hex>/<64-hex>.json`).
fn sweep_blob_root(state_dir: &Path, out: &mut StorageDiagnosis) {
    let blobs_root = state_dir.join("blobs");
    if !blobs_root.is_dir() {
        return;
    }

    let objects = count_files_in(&blobs_root.join("objects"));
    let mut legacy = 0usize;
    if let Ok(entries) = std::fs::read_dir(&blobs_root) {
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let is_shard_dir = path.is_dir()
                && name.len() == 2
                && name
                    .bytes()
                    .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'));
            if is_shard_dir {
                legacy += count_files_in(&path);
            }
        }
    }

    out.findings.push(
        StorageFinding::new(
            FindingSeverity::Info,
            FINDING_BLOB_ROOT,
            format!("blob root ({objects} object(s), {legacy} legacy-layout file(s))"),
        )
        .with_path(blobs_root.clone()),
    );
    if legacy > 0 {
        out.findings.push(
            StorageFinding::new(
                FindingSeverity::Info,
                FINDING_LEGACY_FS_BLOBS,
                format!(
                    "{legacy} blob object(s) remain in the legacy sharded FS layout \
                     (readable through the legacy fallback; migration lands in Phase M6)"
                ),
            )
            .with_path(blobs_root),
        );
    }
}

fn count_files_in(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_file())
                .count()
        })
        .unwrap_or(0)
}

/// Per-realm memory database inventory inside one agent-memory root
/// (`<root>/<pct-encoded-realm>.sqlite3`, feature-owned relative layout).
fn sweep_memory_root(
    memory_root: &Path,
    entry: &mut StorageInventoryEntry,
    out: &mut StorageDiagnosis,
) {
    let Ok(entries) = std::fs::read_dir(memory_root) else {
        return;
    };
    let mut realm_dbs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("sqlite3")
        })
        .collect();
    realm_dbs.sort();
    for db_path in realm_dbs {
        entry
            .databases
            .push(inspect_database(&db_path, &[MEMORY_LEDGER_DOMAIN], out));
    }
}

/// Filesystem artifact sweep: `*.mfence` fence locks, `*.pre-*` migration
/// backups, `*.corrupt-*` quarantines (all inventory-grade).
fn sweep_artifacts(dir: &Path, out: &mut StorageDiagnosis) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    files.sort();
    for file in files {
        let Some(name) = file.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if name.ends_with(".mfence") {
            out.findings.push(
                StorageFinding::new(
                    FindingSeverity::Info,
                    FINDING_MAINTENANCE_FENCE_LOCK,
                    "maintenance-fence lock file (created by normal per-operation guards; held \
                     exclusively only during offline maintenance)",
                )
                .with_path(file.clone()),
            );
        } else if crate::storage_migrate::is_registered_backup_artifact_name(name) {
            // Strict full-shape validation, shared with the prune verb — a
            // loose `.pre-` substring match would misreport user files like
            // `notes.pre-release` as registered backups.
            out.findings.push(
                StorageFinding::new(
                    FindingSeverity::Info,
                    FINDING_BACKUP_ARTIFACT,
                    "migration backup artifact (`*.pre-<version>-<timestamp>`)",
                )
                .with_path(file.clone()),
            );
        } else if crate::storage_migrate::is_registered_quarantine_artifact_name(name) {
            out.findings.push(
                StorageFinding::new(
                    FindingSeverity::Info,
                    FINDING_QUARANTINE_ARTIFACT,
                    "quarantined corrupt file (kept for inspection)",
                )
                .with_path(file.clone()),
            );
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::storage_health::BlobDurability;
    use meerkat_core::{Message, Session, UserMessage};

    fn scope(roots: &[&Path]) -> DiagnoseScope {
        DiagnoseScope::new(roots.iter().map(|root| root.to_path_buf()).collect())
    }

    fn codes(diagnosis: &StorageDiagnosis) -> Vec<&str> {
        diagnosis.findings.iter().map(|f| f.code.as_str()).collect()
    }

    fn create_db_with_table(path: &Path, ddl: &str) {
        let conn = Connection::open(path).unwrap();
        conn.execute_batch(ddl).unwrap();
    }

    const CONTINUITY_DDL: &str = "CREATE TABLE session_snapshots (
        session_id     TEXT PRIMARY KEY,
        identity       TEXT NOT NULL,
        generation     INTEGER NOT NULL,
        checkpoint_version INTEGER NOT NULL,
        fencing_token  INTEGER NOT NULL,
        data           BLOB NOT NULL
    )";

    const CONSOLE_DDL: &str = "CREATE TABLE console_frames (
        cursor_seq INTEGER PRIMARY KEY AUTOINCREMENT,
        id TEXT NOT NULL UNIQUE,
        dedupe_key TEXT NOT NULL UNIQUE,
        payload_json TEXT NOT NULL
    )";

    fn insert_snapshot(conn: &Connection, session_id: &str, identity: &str, data: &[u8]) {
        conn.execute(
            "INSERT INTO session_snapshots (session_id, identity, generation, \
             checkpoint_version, fencing_token, data) VALUES (?1, ?2, 1, 1, 1, ?3)",
            rusqlite::params![session_id, identity, data],
        )
        .unwrap();
    }

    fn unstamped_session_payload() -> (String, Vec<u8>) {
        let mut session = Session::new();
        session.push(Message::User(UserMessage::text("hello")));
        (
            session.id().to_string(),
            serde_json::to_vec(&session).unwrap(),
        )
    }

    #[tokio::test]
    async fn healthy_state_dir_inventories_without_errors() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        create_db_with_table(
            &state.join("sessions.db"),
            "CREATE TABLE sessions (session_id TEXT PRIMARY KEY)",
        );
        create_db_with_table(
            &state.join("runtime.sqlite"),
            "CREATE TABLE runtime_rows (id TEXT PRIMARY KEY)",
        );
        let objects = state.join("blobs").join("objects");
        std::fs::create_dir_all(&objects).unwrap();
        std::fs::write(objects.join(format!("{}.bin", "a".repeat(64))), b"x").unwrap();
        std::fs::write(state.join(GATEWAY_PEER_KEY_FILE), [0u8; 32]).unwrap();
        std::fs::write(state.join(RUNTIME_REGISTRY_FILE), b"{}").unwrap();

        let diagnosis = diagnose_state_dir(&scope(&[state])).await;
        assert!(!diagnosis.has_errors(), "{diagnosis:?}");
        assert_eq!(diagnosis.inventory.len(), 1);
        assert_eq!(diagnosis.inventory[0].databases.len(), 2);
        let found = codes(&diagnosis);
        for expected in [
            FINDING_NO_SCHEMA_LEDGER,
            FINDING_BLOB_ROOT,
            FINDING_PEER_KEY_FILE,
            FINDING_RUNTIME_REGISTRY,
            FINDING_DURABILITY_CENSUS_UNAVAILABLE,
        ] {
            assert!(found.contains(&expected), "missing {expected}: {found:?}");
        }
        assert!(!found.contains(&FINDING_FILE_NAME_TWINS));
    }

    #[tokio::test]
    async fn file_name_twins_detected_for_databases_and_memory_roots() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        create_db_with_table(&state.join("sessions.db"), "CREATE TABLE s (id TEXT)");
        create_db_with_table(&state.join("sessions.sqlite"), "CREATE TABLE s (id TEXT)");
        create_db_with_table(&state.join("continuity.db"), CONTINUITY_DDL);
        create_db_with_table(&state.join("identity_continuity.sqlite"), CONTINUITY_DDL);
        std::fs::create_dir_all(state.join("agent-memory")).unwrap();
        std::fs::create_dir_all(state.join("agent-memory-sqlite")).unwrap();

        let diagnosis = diagnose_state_dir(&scope(&[state])).await;
        let twins: Vec<_> = diagnosis
            .findings
            .iter()
            .filter(|f| f.code == FINDING_FILE_NAME_TWINS)
            .collect();
        assert_eq!(twins.len(), 3, "{diagnosis:?}");
        assert!(twins.iter().all(|f| f.severity == FindingSeverity::Error));
        assert!(twins.iter().any(|f| f.message.contains("sessions")));
        assert!(twins.iter().any(|f| f.message.contains("continuity")));
        assert!(twins.iter().any(|f| f.message.contains("agent-memory")));
        // Every twin is still inventoried individually.
        assert_eq!(diagnosis.inventory[0].databases.len(), 4);
    }

    #[tokio::test]
    async fn legacy_spelling_alone_is_not_a_twin() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        create_db_with_table(&state.join("sessions.sqlite"), "CREATE TABLE s (id TEXT)");
        create_db_with_table(&state.join("continuity.db"), CONTINUITY_DDL);

        let diagnosis = diagnose_state_dir(&scope(&[state])).await;
        assert!(!codes(&diagnosis).contains(&FINDING_FILE_NAME_TWINS));
        assert!(!diagnosis.has_errors(), "{diagnosis:?}");
        assert_eq!(diagnosis.inventory[0].databases.len(), 2);
    }

    #[tokio::test]
    async fn continuity_census_counts_unstamped_snapshots_per_identity() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        let db_path = state.join("continuity.db");
        create_db_with_table(&db_path, CONTINUITY_DDL);
        {
            let conn = Connection::open(&db_path).unwrap();
            let (sid_a, data_a) = unstamped_session_payload();
            insert_snapshot(&conn, &sid_a, "domain:security", &data_a);
            let (sid_b, data_b) = unstamped_session_payload();
            insert_snapshot(&conn, &sid_b, "domain:security", &data_b);
            insert_snapshot(&conn, "sid-garbage", "domain:ops", b"not-a-session");
        }

        let diagnosis = diagnose_state_dir(&scope(&[state])).await;
        let legacy = diagnosis
            .findings
            .iter()
            .find(|f| f.code == FINDING_LEGACY_UNVERIFIED_CONTINUITY_SNAPSHOTS)
            .expect("legacy census finding");
        assert_eq!(legacy.severity, FindingSeverity::Warning);
        assert!(
            legacy.message.starts_with("2 legacy-unverified"),
            "{}",
            legacy.message
        );
        assert_eq!(legacy.realm.as_deref(), Some("domain:security"));
        let undecodable = diagnosis
            .findings
            .iter()
            .find(|f| f.code == FINDING_CONTINUITY_SNAPSHOT_UNDECODABLE)
            .expect("undecodable finding");
        assert!(undecodable.message.starts_with("1 snapshot"));

        // The realm filter narrows the census to one identity.
        let filtered = diagnose_state_dir(&scope(&[state]).with_realm("domain:ops")).await;
        assert!(
            !codes(&filtered).contains(&FINDING_LEGACY_UNVERIFIED_CONTINUITY_SNAPSHOTS),
            "{filtered:?}"
        );
        assert!(codes(&filtered).contains(&FINDING_CONTINUITY_SNAPSHOT_UNDECODABLE));
    }

    /// A session with a verified root checkpoint stamp installed.
    fn stamped_session() -> Session {
        let mut session = Session::new();
        session.push(Message::User(UserMessage::text("stamped")));
        let stamp = meerkat_core::SessionCheckpointStamp::root(
            &session,
            meerkat_core::SessionCheckpointProvenance::SessionCreated,
        )
        .expect("root stamp");
        session
            .install_checkpoint_stamp(stamp)
            .expect("install stamp");
        session
    }

    #[tokio::test]
    async fn continuity_census_verifies_stamped_checkpoint_digests() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        let db_path = state.join("continuity.db");
        create_db_with_table(&db_path, CONTINUITY_DDL);

        // One verified stamped snapshot, and one whose content changed after
        // stamping — structurally stamped, but the digest no longer matches
        // the bytes (restore rejects it; the doctor must not call it clean).
        let good = stamped_session();
        let (good_sid, good_bytes) = (
            good.id().to_string(),
            serde_json::to_vec(&good).expect("serialize"),
        );
        let mut tampered = stamped_session();
        tampered.push(Message::User(UserMessage::text("tampered after stamping")));
        let (bad_sid, bad_bytes) = (
            tampered.id().to_string(),
            serde_json::to_vec(&tampered).expect("serialize"),
        );
        {
            let conn = Connection::open(&db_path).unwrap();
            insert_snapshot(&conn, &good_sid, "domain:good", &good_bytes);
            insert_snapshot(&conn, &bad_sid, "domain:bad", &bad_bytes);
        }

        let diagnosis = diagnose_state_dir(&scope(&[state])).await;
        let mismatches: Vec<_> = diagnosis
            .findings
            .iter()
            .filter(|f| f.code == FINDING_CHECKPOINT_DIGEST_MISMATCH)
            .collect();
        assert_eq!(mismatches.len(), 1, "{diagnosis:?}");
        assert_eq!(mismatches[0].severity, FindingSeverity::Error);
        assert!(
            mismatches[0].message.contains(&bad_sid),
            "the finding must name the session: {}",
            mismatches[0].message
        );
        assert_eq!(mismatches[0].realm.as_deref(), Some("domain:bad"));
        assert!(!mismatches[0].message.contains(&good_sid));
        assert!(diagnosis.has_errors());
    }

    #[tokio::test]
    async fn ledger_state_reported_with_and_without_ledger_table() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        create_db_with_table(&state.join("continuity.db"), CONTINUITY_DDL);
        create_db_with_table(
            &state.join("mobkit_metadata.sqlite"),
            "CREATE TABLE meerkat_schema (domain TEXT PRIMARY KEY, version INTEGER NOT NULL);
             INSERT INTO meerkat_schema (domain, version) VALUES ('mobkit-metadata', 3);
             INSERT INTO meerkat_schema (domain, version) VALUES ('surprise-domain', 7);",
        );

        let diagnosis = diagnose_state_dir(&scope(&[state])).await;
        assert!(codes(&diagnosis).contains(&FINDING_NO_SCHEMA_LEDGER));
        let entry = &diagnosis.inventory[0];
        let continuity = entry
            .databases
            .iter()
            .find(|db| db.path.ends_with("continuity.db"))
            .expect("continuity inventory");
        assert_eq!(
            continuity.domains,
            vec![("mobkit-continuity".to_string(), None)]
        );
        let metadata = entry
            .databases
            .iter()
            .find(|db| db.path.ends_with("mobkit_metadata.sqlite"))
            .expect("metadata inventory");
        assert!(
            metadata
                .domains
                .contains(&("mobkit-metadata".to_string(), Some(3)))
        );
        assert!(
            metadata
                .domains
                .contains(&("surprise-domain".to_string(), Some(7)))
        );
    }

    #[tokio::test]
    async fn dangling_console_blob_reference_detected() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        let missing = format!("sha256:{}", "a".repeat(64));
        let present = format!("sha256:{}", "b".repeat(64));
        let objects = state.join("blobs").join("objects");
        std::fs::create_dir_all(&objects).unwrap();
        std::fs::write(objects.join(format!("{}.bin", "b".repeat(64))), b"x").unwrap();
        let db_path = state.join("mobkit_console.sqlite");
        create_db_with_table(&db_path, CONSOLE_DDL);
        {
            let conn = Connection::open(&db_path).unwrap();
            for (idx, blob_id) in [&missing, &present].into_iter().enumerate() {
                conn.execute(
                    "INSERT INTO console_frames (id, dedupe_key, payload_json) \
                     VALUES (?1, ?2, ?3)",
                    rusqlite::params![
                        format!("frame-{idx}"),
                        format!("dedupe-{idx}"),
                        serde_json::json!({ "blob_id": blob_id }).to_string(),
                    ],
                )
                .unwrap();
            }
        }

        let diagnosis = diagnose_state_dir(&scope(&[state])).await;
        let dangling: Vec<_> = diagnosis
            .findings
            .iter()
            .filter(|f| f.code == FINDING_DANGLING_CONSOLE_BLOB_REFERENCE)
            .collect();
        assert_eq!(dangling.len(), 1, "{diagnosis:?}");
        assert!(dangling[0].message.contains(&missing));
        assert!(!dangling[0].message.contains(&present));
        assert!(diagnosis.has_errors());
    }

    #[tokio::test]
    async fn artifact_and_sidecar_findings_are_informational() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        create_db_with_table(
            &state.join(WORKGRAPH_ADMISSION_SIDECAR_FILE),
            "CREATE TABLE admission_lock (id INTEGER PRIMARY KEY)",
        );
        std::fs::write(state.join("sessions.db.mfence"), b"").unwrap();
        std::fs::write(state.join("sessions.db.pre-0.0.1-1700000000"), b"backup").unwrap();
        std::fs::write(state.join("continuity.db.corrupt-123"), b"x").unwrap();
        // Lookalikes outside the registered full shape are never reported
        // as maintenance artifacts.
        std::fs::write(state.join("notes.pre-release"), b"user file").unwrap();
        std::fs::write(state.join("report.corrupt-12a"), b"user file").unwrap();
        let legacy_shard = state.join("blobs").join("aa");
        std::fs::create_dir_all(&legacy_shard).unwrap();
        std::fs::write(legacy_shard.join(format!("{}.json", "a".repeat(64))), b"{}").unwrap();

        let diagnosis = diagnose_state_dir(&scope(&[state])).await;
        let found = codes(&diagnosis);
        for expected in [
            FINDING_WORKGRAPH_ADMISSION_SIDECAR,
            FINDING_MAINTENANCE_FENCE_LOCK,
            FINDING_BACKUP_ARTIFACT,
            FINDING_QUARANTINE_ARTIFACT,
            FINDING_LEGACY_FS_BLOBS,
        ] {
            assert!(found.contains(&expected), "missing {expected}: {found:?}");
        }
        // Exactly the strictly-shaped artifacts are reported; the lookalike
        // user files are not.
        let count_of = |code: &str| found.iter().filter(|found| **found == code).count();
        assert_eq!(count_of(FINDING_BACKUP_ARTIFACT), 1, "{found:?}");
        assert_eq!(count_of(FINDING_QUARANTINE_ARTIFACT), 1, "{found:?}");
        assert!(!diagnosis.has_errors(), "{diagnosis:?}");
    }

    #[tokio::test]
    async fn live_durability_census_attaches_resolved_summary() {
        let temp = tempfile::tempdir().unwrap();
        let summary = ResolvedStorageSummary::new(BlobDurability::PersistentDisk, Some(true));
        let diagnosis =
            diagnose_state_dir_with_runtime(&scope(&[temp.path()]), Some(summary)).await;
        let found = codes(&diagnosis);
        assert!(found.contains(&FINDING_BLOB_DURABILITY));
        assert!(found.contains(&FINDING_SESSION_STORE_INCREMENTAL));
        assert!(!found.contains(&FINDING_DURABILITY_CENSUS_UNAVAILABLE));
        let blob = diagnosis
            .findings
            .iter()
            .find(|f| f.code == FINDING_BLOB_DURABILITY)
            .expect("blob durability finding");
        assert!(blob.message.contains("persistent_disk"));
    }

    #[tokio::test]
    async fn explicit_roots_are_the_only_thing_read_and_missing_roots_reported() {
        let temp = tempfile::tempdir().unwrap();
        let scoped = temp.path().join("scoped");
        let unscoped = temp.path().join("unscoped");
        std::fs::create_dir_all(&scoped).unwrap();
        std::fs::create_dir_all(&unscoped).unwrap();
        create_db_with_table(&scoped.join("sessions.db"), "CREATE TABLE s (id TEXT)");
        create_db_with_table(&unscoped.join("sessions.db"), "CREATE TABLE s (id TEXT)");

        let diagnosis = diagnose_state_dir(&scope(&[&scoped])).await;
        assert_eq!(diagnosis.inventory.len(), 1);
        assert_eq!(diagnosis.inventory[0].root, scoped);

        let missing = temp.path().join("nope");
        let diagnosis = diagnose_state_dir(&scope(&[&missing])).await;
        assert!(codes(&diagnosis).contains(&FINDING_STATE_ROOT_MISSING));
        assert!(diagnosis.inventory.is_empty());
    }

    #[tokio::test]
    async fn empty_shell_databases_are_flagged() {
        let temp = tempfile::tempdir().unwrap();
        let state = temp.path();
        // A zero-table database file.
        drop(Connection::open(state.join("schedule.sqlite")).unwrap());

        let diagnosis = diagnose_state_dir(&scope(&[state])).await;
        let shell = diagnosis
            .findings
            .iter()
            .find(|f| f.code == FINDING_EMPTY_DATABASE_SHELL)
            .expect("empty shell finding");
        assert_eq!(shell.severity, FindingSeverity::Info);
        assert!(
            shell
                .path
                .as_ref()
                .is_some_and(|p| p.ends_with("schedule.sqlite"))
        );
    }

    #[tokio::test]
    async fn storage_migrator_delegates() {
        let temp = tempfile::tempdir().unwrap();
        create_db_with_table(&temp.path().join("sessions.db"), "CREATE TABLE s (id TEXT)");
        let migrator = MobKitStorageMigrator;
        let diagnosis = migrator
            .diagnose(&scope(&[temp.path()]))
            .await
            .expect("diagnose never fails on disk");
        assert_eq!(diagnosis.inventory.len(), 1);
    }
}
