//! In-repo instantiations: every chapter runs against the bundled MobKit
//! implementations (or, where MobKit bundles no production store for a
//! trait, against the in-crate reference implementation that proves the
//! chapter satisfiable).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use meerkat_mobkit::blob_store::{BinaryBlobStore, ObjectStoreBlobStore};
use meerkat_mobkit::console_aggregator::{
    ConsoleLogStore, InMemoryConsoleLogStore, SqliteConsoleLogStore,
};
use meerkat_mobkit::identity_first::{
    AgentMemoryProvider, ContinuityStore, LocalContinuityStore, MarkdownAgentMemoryStore,
};
use meerkat_mobkit::memory::SqliteAgentMemoryStore;
use meerkat_mobkit::unified_runtime::EventLogStore;
use mobkit_store_conformance::{
    AgentMemoryProviderFactory, BinaryBlobStoreFactory, CompatRollbackContinuityStore,
    ConformanceFailure, ConsoleLogStoreFactory, ContinuityStoreFactory, EventLogStoreFactory,
    ReferenceEventLogStore, chapters,
};

// ---------------------------------------------------------------------------
// Factories over the bundled implementations
// ---------------------------------------------------------------------------

/// File-backed `LocalContinuityStore`: each open is a NEW handle over the
/// same database file (models a process restart).
struct LocalFileContinuityFactory {
    path: PathBuf,
}

#[async_trait]
impl ContinuityStoreFactory for LocalFileContinuityFactory {
    async fn open(&self) -> Result<Arc<dyn ContinuityStore>, ConformanceFailure> {
        let store = LocalContinuityStore::open(&self.path)
            .map_err(|error| ConformanceFailure::new("factory", "open", error.to_string()))?;
        Ok(Arc::new(store))
    }
}

/// Shared-handle factory for deliberately non-persistent continuity stores.
struct SharedContinuityFactory {
    store: Arc<dyn ContinuityStore>,
}

#[async_trait]
impl ContinuityStoreFactory for SharedContinuityFactory {
    async fn open(&self) -> Result<Arc<dyn ContinuityStore>, ConformanceFailure> {
        Ok(Arc::clone(&self.store))
    }
}

struct SqliteConsoleFactory {
    path: PathBuf,
}

#[async_trait]
impl ConsoleLogStoreFactory for SqliteConsoleFactory {
    async fn open(&self) -> Result<Arc<dyn ConsoleLogStore>, ConformanceFailure> {
        let store = SqliteConsoleLogStore::open(&self.path)
            .map_err(|error| ConformanceFailure::new("factory", "open", error.to_string()))?;
        Ok(Arc::new(store))
    }
}

struct SharedConsoleFactory {
    store: Arc<dyn ConsoleLogStore>,
}

#[async_trait]
impl ConsoleLogStoreFactory for SharedConsoleFactory {
    async fn open(&self) -> Result<Arc<dyn ConsoleLogStore>, ConformanceFailure> {
        Ok(Arc::clone(&self.store))
    }
}

struct SharedEventLogFactory {
    store: Arc<dyn EventLogStore>,
}

#[async_trait]
impl EventLogStoreFactory for SharedEventLogFactory {
    async fn open(&self) -> Result<Arc<dyn EventLogStore>, ConformanceFailure> {
        Ok(Arc::clone(&self.store))
    }
}

struct LocalBlobFactory {
    root: PathBuf,
}

#[async_trait]
impl BinaryBlobStoreFactory for LocalBlobFactory {
    async fn open(&self) -> Result<Arc<dyn BinaryBlobStore>, ConformanceFailure> {
        let store = ObjectStoreBlobStore::local(self.root.clone())
            .map_err(|error| ConformanceFailure::new("factory", "open", error.to_string()))?;
        Ok(Arc::new(store))
    }
}

struct SharedBlobFactory {
    store: Arc<dyn BinaryBlobStore>,
}

#[async_trait]
impl BinaryBlobStoreFactory for SharedBlobFactory {
    async fn open(&self) -> Result<Arc<dyn BinaryBlobStore>, ConformanceFailure> {
        Ok(Arc::clone(&self.store))
    }
}

struct SqliteMemoryFactory {
    root: PathBuf,
}

#[async_trait]
impl AgentMemoryProviderFactory for SqliteMemoryFactory {
    async fn open(&self) -> Result<Arc<dyn AgentMemoryProvider>, ConformanceFailure> {
        let store = SqliteAgentMemoryStore::open(&self.root)
            .map_err(|error| ConformanceFailure::new("factory", "open", error.to_string()))?;
        Ok(Arc::new(store))
    }
}

struct MarkdownMemoryFactory {
    root: PathBuf,
}

#[async_trait]
impl AgentMemoryProviderFactory for MarkdownMemoryFactory {
    async fn open(&self) -> Result<Arc<dyn AgentMemoryProvider>, ConformanceFailure> {
        let store = MarkdownAgentMemoryStore::open(&self.root)
            .map_err(|error| ConformanceFailure::new("factory", "open", error.to_string()))?;
        Ok(Arc::new(store))
    }
}

// ---------------------------------------------------------------------------
// ContinuityStore profile
// ---------------------------------------------------------------------------

#[tokio::test]
async fn local_continuity_store_file_backed_passes_continuity_profile() {
    let dir = tempfile::tempdir().expect("tempdir");
    let factory = LocalFileContinuityFactory {
        path: dir.path().join("continuity.sqlite"),
    };
    chapters::continuity_store(&factory)
        .await
        .expect("LocalContinuityStore (file) must satisfy the continuity profile");
}

#[tokio::test]
async fn local_continuity_store_in_memory_passes_continuity_profile() {
    let store = LocalContinuityStore::in_memory().expect("in-memory store");
    let factory = SharedContinuityFactory {
        store: Arc::new(store),
    };
    chapters::continuity_store(&factory)
        .await
        .expect("LocalContinuityStore (memory) must satisfy the continuity profile");
}

#[tokio::test]
async fn compat_reference_store_passes_continuity_profile() {
    let factory = SharedContinuityFactory {
        store: Arc::new(CompatRollbackContinuityStore::new()),
    };
    chapters::continuity_store(&factory)
        .await
        .expect("the in-crate reference store must prove the continuity profile satisfiable");
}

#[tokio::test]
async fn local_continuity_store_rollback_atomic_override() {
    let dir = tempfile::tempdir().expect("tempdir");
    let factory = LocalFileContinuityFactory {
        path: dir.path().join("continuity.sqlite"),
    };
    chapters::continuity_rollback(&factory, chapters::RollbackPath::AtomicOverride)
        .await
        .expect("LocalContinuityStore must satisfy the atomic rollback profile");
}

/// Exercises the trait's NON-ATOMIC compatibility rollback (the reference
/// store deliberately does not override it). M4b fixed the default —
/// delete-then-reinsert restore a conforming generation-monotonic store CAN
/// satisfy — and this pins the fixed behavior, including the documented
/// caveat that the identity's snapshots (previous generation's included) do
/// not survive the compatibility path.
#[tokio::test]
async fn compat_reference_store_rollback_default_impl() {
    let factory = SharedContinuityFactory {
        store: Arc::new(CompatRollbackContinuityStore::new()),
    };
    chapters::continuity_rollback(&factory, chapters::RollbackPath::CompatibilityDefault)
        .await
        .expect("the compatibility rollback pins must hold on the default-impl store");
}

#[tokio::test]
async fn local_continuity_fencing_floor_survives_reopen() {
    let dir = tempfile::tempdir().expect("tempdir");
    chapters::local_continuity_fencing_floor(dir.path())
        .await
        .expect("the bundled LocalContinuityStore + LocalLeaseProvider pair must keep the floor");
}

// ---------------------------------------------------------------------------
// ContinuitySessionStoreAdapter
// ---------------------------------------------------------------------------

/// H2 pin, kept after M4b's DELIBERATE deferral: the capability seam
/// (`ContinuityStore::as_incremental_sessions`) and the adapter forwarding
/// landed, but the bundled `LocalContinuityStore` returns `None` — shipping
/// a store-side delta channel beside the whole-snapshot byte authority
/// (exact-match probes, parse-derived CAS tokens, `checkpoint_session`,
/// H3's lazy adoption byte custody) would create two write authorities over
/// one session with no reconciliation rule. Every identity-first deployment
/// on the bundled store therefore still persists whole session documents
/// per save; this pin flips when the local snapshot representation itself
/// moves to head+rows (gated by meerkat-store-conformance's incremental
/// profile).
#[tokio::test]
async fn continuity_session_adapter_over_local_store() {
    let dir = tempfile::tempdir().expect("tempdir");
    let factory = LocalFileContinuityFactory {
        path: dir.path().join("continuity.sqlite"),
    };
    chapters::continuity_session_adapter(&factory, false)
        .await
        .expect("ContinuitySessionStoreAdapter must satisfy the adapter chapter");
}

/// Canary: asserting the flipped world today must FAIL at the capability
/// step — proving the pin actually flips instead of passing vacuously.
/// (Written pre-M4b to flip with the incremental channel; it stays pinned
/// false because M4b deferred the bundled store's channel — see the pin
/// above for why.)
#[tokio::test]
async fn continuity_session_adapter_incremental_expectation_fails_today() {
    let dir = tempfile::tempdir().expect("tempdir");
    let factory = LocalFileContinuityFactory {
        path: dir.path().join("continuity.sqlite"),
    };
    let failure = chapters::continuity_session_adapter(&factory, true)
        .await
        .expect_err("expecting the incremental capability today must fail");
    assert_eq!(failure.chapter(), "continuity_session_adapter");
    assert_eq!(failure.step(), "as_incremental_capability");
}

// ---------------------------------------------------------------------------
// EventLogStore profile
// ---------------------------------------------------------------------------

/// MobKit bundles no durable production `EventLogStore` (the built-in
/// default is a private null store that drops every event; the rpc_gateway's
/// in-memory store is binary-local). The in-crate reference store proves the
/// profile satisfiable, exactly like the upstream emulated-CAS reference.
#[tokio::test]
async fn reference_event_log_store_satisfies_event_log_profile() {
    let factory = SharedEventLogFactory {
        store: Arc::new(ReferenceEventLogStore::new()),
    };
    chapters::event_log(&factory)
        .await
        .expect("the reference event log store must satisfy the event log profile");
}

// ---------------------------------------------------------------------------
// ConsoleLogStore profile
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn sqlite_console_store_passes_console_profile() {
    let dir = tempfile::tempdir().expect("tempdir");
    let factory = SqliteConsoleFactory {
        path: dir.path().join("mobkit_console.sqlite"),
    };
    chapters::console_log(&factory)
        .await
        .expect("SqliteConsoleLogStore must satisfy the console profile");
}

#[tokio::test(flavor = "multi_thread")]
async fn in_memory_console_store_passes_console_profile() {
    let factory = SharedConsoleFactory {
        store: Arc::new(InMemoryConsoleLogStore::new()),
    };
    chapters::console_log(&factory)
        .await
        .expect("InMemoryConsoleLogStore must satisfy the console profile");
}

// ---------------------------------------------------------------------------
// BinaryBlobStore profile
// ---------------------------------------------------------------------------

#[tokio::test]
async fn local_blob_store_passes_binary_blobs_profile() {
    let dir = tempfile::tempdir().expect("tempdir");
    let factory = LocalBlobFactory {
        root: dir.path().join("blobs"),
    };
    chapters::binary_blobs(&factory, Some(true))
        .await
        .expect("ObjectStoreBlobStore::local must satisfy the binary blob profile");
}

/// The memory backend must answer `is_persistent() == false` — a memory
/// store claiming persistence is the exact silent-loss hazard H1 deletes.
#[tokio::test]
async fn memory_blob_store_passes_binary_blobs_profile() {
    let factory = SharedBlobFactory {
        store: Arc::new(ObjectStoreBlobStore::memory()),
    };
    chapters::binary_blobs(&factory, Some(false))
        .await
        .expect("ObjectStoreBlobStore::memory must satisfy the binary blob profile");
}

#[tokio::test]
async fn local_blob_store_reads_legacy_fs_layout() {
    let dir = tempfile::tempdir().expect("tempdir");
    chapters::legacy_blob_layout(dir.path())
        .await
        .expect("the legacy filesystem blob layout must stay readable");
}

// ---------------------------------------------------------------------------
// AgentMemoryProvider capability matrix
// ---------------------------------------------------------------------------

#[tokio::test]
async fn sqlite_agent_memory_store_passes_capability_matrix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let factory = SqliteMemoryFactory {
        root: dir.path().to_path_buf(),
    };
    chapters::agent_memory(&factory)
        .await
        .expect("SqliteAgentMemoryStore (all-capable) must satisfy the capability matrix");
}

/// The recall-only-by-flags shape: remember/forget advertised, everything
/// else refused with the typed Unsupported error.
#[tokio::test]
async fn markdown_agent_memory_store_passes_capability_matrix() {
    let dir = tempfile::tempdir().expect("tempdir");
    let factory = MarkdownMemoryFactory {
        root: dir.path().to_path_buf(),
    };
    chapters::agent_memory(&factory)
        .await
        .expect("MarkdownAgentMemoryStore must satisfy the capability matrix");
}

// ---------------------------------------------------------------------------
// Legacy-data axis
// ---------------------------------------------------------------------------

#[tokio::test]
async fn legacy_continuity_database_opens_and_preserves_bytes() {
    let dir = tempfile::tempdir().expect("tempdir");
    chapters::legacy_continuity_database(dir.path())
        .await
        .expect("a pre-ledger continuity database must open with bytes and CAS intact");
}

#[tokio::test]
async fn legacy_memory_database_upgrades_preserving_rows_and_sentinel() {
    let dir = tempfile::tempdir().expect("tempdir");
    chapters::legacy_memory_database(dir.path())
        .await
        .expect("a pre-column memory database must upgrade preserving rows and the sentinel");
}
