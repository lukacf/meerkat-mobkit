//! The MobKit composite storage-provider seam (Phase M4).
//!
//! Meerkat's [`RealmStorageProvider`] supplies the meerkat-shared stores
//! (sessions, runtime, schedule, workgraph, blobs, artifacts) and cannot
//! name MobKit-owned types without reversing the crate dependency. MobKit
//! therefore defines its own composite seam: a [`MobKitStorageProvider`]
//! *wraps or references* a [`RealmStorageProvider`] for the meerkat-shared
//! level and additionally opens the **realm-wide** MobKit store set —
//! continuity, lease-fencing authority, event log, console timeline,
//! metadata, blobs, agent memory, schedule — each slot paired with a
//! machine-readable [`DurabilityDeclaration`]. These are realm-wide stores
//! opened once at bootstrap, not per-mob objects.
//!
//! **One remote bundle, one seam**: a downstream backend (BigQuery,
//! Postgres, object stores) implements one `MobKitStorageProvider` and
//! covers both levels — sessions, continuity, events, blobs, and memory
//! flow through a single integration surface, with
//! `mobkit-store-conformance` (plus meerkat's conformance profiles via
//! [`MobKitStorageProvider::meerkat_provider`]) as the acceptance suite.
//!
//! **Fail-closed composition**: every durable slot in the returned set
//! resolves to provider-supplied storage, or an *explicitly declared*
//! ephemeral choice, or the open fails typed — never a silent in-memory
//! fallback ([`enforce_fail_closed_store_set`]).

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use meerkat::storage_provider::RealmStorageProvider;
use meerkat_core::{DurabilityDeclaration, DurabilityResolution};

use crate::blob_store::{BinaryBlobStore, ObjectStoreBlobStore};
use crate::console_aggregator::{ConsoleLogStore, SqliteConsoleLogStore};
use crate::identity_first::contracts::{ContinuityStore, LeaseProvider};
use crate::identity_first::{AgentMemoryProvider, LocalContinuityStore};
use crate::memory::sqlite_store::SqliteAgentMemoryStore;
use crate::runtime::{PersistentMetadataStore, SqliteMetadataStore};
use crate::storage_doctor::MobKitStorageMigrator;
use crate::storage_layout::MobKitStorageLayout;
use crate::unified_runtime::EventLogStore;

/// Everything a provider needs to open a realm's MobKit stores.
#[derive(Clone)]
pub struct MobKitRealmOpenContext {
    /// The path authority for the realm's state directory (canonical-name-
    /// first probing; disk providers derive every file location from it).
    pub layout: MobKitStorageLayout,
    /// The realm's state directory root (`layout.state_dir()`, owned).
    pub state_dir: PathBuf,
    /// Durable-class domains the embedder explicitly declared ephemeral
    /// (`"blobs"`, `"runtime"`, ...). A durable slot resolving non-persistent
    /// without its domain listed here fails composition.
    pub declared_ephemeral_domains: Vec<String>,
}

impl MobKitRealmOpenContext {
    /// Context over a state directory with no ephemeral declarations.
    pub fn for_state_dir(state_dir: impl Into<PathBuf>) -> Self {
        let state_dir = state_dir.into();
        Self {
            layout: MobKitStorageLayout::with_injected_roots(state_dir.clone(), None),
            state_dir,
            declared_ephemeral_domains: Vec::new(),
        }
    }

    /// Whether `domain` was explicitly declared ephemeral.
    pub fn is_declared_ephemeral(&self, domain: &str) -> bool {
        self.declared_ephemeral_domains
            .iter()
            .any(|declared| declared == domain)
    }
}

/// Live-ownership authority for the realm: either a full [`LeaseProvider`]
/// or the persisted fencing-token floor the composition seeds the bundled
/// `LocalLeaseProvider::with_floor` from (the disk provider's shape — its
/// lease state is process-local, only the floor is durable).
#[derive(Clone)]
pub enum MobKitLeaseAuthority {
    Provider(Arc<dyn LeaseProvider>),
    FencingFloor(u64),
}

/// The realm-wide MobKit stores a provider supplies, each slot covered by a
/// [`DurabilityDeclaration`] in [`Self::durability`].
///
/// `event_log_store` and `agent_memory_provider` are optional features:
/// `None` means the provider supplies no backend for them (the composition
/// keeps its declared defaults — no event ingestion / no agent memory) and
/// the corresponding declaration records that as an explicit choice.
pub struct MobKitRealmStoreSet {
    pub continuity_store: Arc<dyn ContinuityStore>,
    pub lease_authority: MobKitLeaseAuthority,
    pub event_log_store: Option<Box<dyn EventLogStore>>,
    pub console_log_store: Arc<dyn ConsoleLogStore>,
    pub metadata_store: Arc<dyn PersistentMetadataStore>,
    pub blob_store: Arc<dyn BinaryBlobStore>,
    pub agent_memory_provider: Option<Arc<dyn AgentMemoryProvider>>,
    pub schedule_store: Arc<dyn meerkat::ScheduleStore>,
    /// Per-slot durability declarations, machine-readable. Domain names:
    /// `continuity`, `event_log`, `console`, `metadata`, `blobs`,
    /// `agent_memory`, `schedule`.
    pub durability: Vec<DurabilityDeclaration>,
}

/// Typed failure opening or validating a realm's MobKit store set.
#[derive(Debug)]
pub enum MobKitStorageProviderError {
    /// A slot's backend failed to open.
    Open { slot: String, message: String },
    /// The fail-closed durability rule refused the set: a `Durable` slot
    /// resolved non-persistent without an explicit ephemeral declaration.
    DurabilityViolation { domain: String },
}

impl std::fmt::Display for MobKitStorageProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open { slot, message } => {
                write!(f, "failed to open the {slot} store: {message}")
            }
            Self::DurabilityViolation { domain } => write!(
                f,
                "durable storage slot '{domain}' resolved to a non-persistent \
                 backend without an explicit ephemeral declaration — declare \
                 the domain ephemeral or supply persistent storage \
                 (fail-closed durability, storage-unification principle 7)"
            ),
        }
    }
}

impl std::error::Error for MobKitStorageProviderError {}

/// One provider supplies all realm-wide MobKit stores plus (via
/// [`Self::meerkat_provider`]) the meerkat-shared stores — the single seam a
/// downstream backend implements.
#[async_trait]
pub trait MobKitStorageProvider: Send + Sync {
    /// Stable provider name (`"disk"` for the built-in implementation).
    fn name(&self) -> &str;

    /// Open (or create) the realm's MobKit stores. Implementations must
    /// apply [`enforce_fail_closed_store_set`] (or an equivalent check)
    /// before returning, so an undeclared non-persistent durable slot is a
    /// typed startup error at the seam.
    async fn open_realm(
        &self,
        ctx: &MobKitRealmOpenContext,
    ) -> Result<MobKitRealmStoreSet, MobKitStorageProviderError>;

    /// The meerkat-level provider for the same backend: sessions, runtime,
    /// schedule, workgraph, blobs, artifacts. One downstream implements one
    /// `MobKitStorageProvider` and covers both levels.
    fn meerkat_provider(&self) -> &dyn RealmStorageProvider;

    /// The provider's storage-maintenance hook (diagnosis now; the mutation
    /// verbs land with the M6 migration framework). `None` = no migration
    /// story yet.
    fn migrator(&self) -> Option<&dyn meerkat_core::StorageMigrator> {
        None
    }
}

/// Every durability domain a [`MobKitRealmStoreSet`] must declare — one
/// declaration per slot, no omissions. Mirrors the meerkat-level
/// `REQUIRED_DURABILITY_DOMAINS` contract for the mobkit slots.
pub const REQUIRED_MOBKIT_DURABILITY_DOMAINS: [&str; 7] = [
    "continuity",
    "event_log",
    "console",
    "metadata",
    "blobs",
    "agent_memory",
    "schedule",
];

/// Enforce the fail-closed durability rule against the context's explicit
/// ephemeral declarations.
///
/// Completeness first: every slot in
/// [`REQUIRED_MOBKIT_DURABILITY_DOMAINS`] must carry exactly one
/// declaration — a provider cannot dodge the rule by omitting a domain or
/// returning an empty list. Then each declaration is checked: a `Durable`
/// slot resolving `NonPersistent` without its domain declared ephemeral
/// refuses composition.
pub fn enforce_fail_closed_store_set(
    set: &MobKitRealmStoreSet,
    ctx: &MobKitRealmOpenContext,
) -> Result<(), MobKitStorageProviderError> {
    for required in REQUIRED_MOBKIT_DURABILITY_DOMAINS {
        let count = set
            .durability
            .iter()
            .filter(|declaration| declaration.domain == required)
            .count();
        if count != 1 {
            return Err(MobKitStorageProviderError::DurabilityViolation {
                domain: format!(
                    "{required} (provider supplied {count} durability declarations for this                      slot; exactly one is required)"
                ),
            });
        }
    }
    for declaration in &set.durability {
        if declaration.is_undeclared_nonpersistent_durable()
            && !ctx.is_declared_ephemeral(&declaration.domain)
        {
            return Err(MobKitStorageProviderError::DurabilityViolation {
                domain: declaration.domain.clone(),
            });
        }
    }
    Ok(())
}

/// The built-in disk provider: reproduces today's SQLite/object-store layout
/// via the M2 [`MobKitStorageLayout`] locators and the M3 ledgered openers.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiskMobKitStorageProvider;

impl DiskMobKitStorageProvider {
    fn open_error(slot: &str, message: impl std::fmt::Display) -> MobKitStorageProviderError {
        MobKitStorageProviderError::Open {
            slot: slot.to_string(),
            message: message.to_string(),
        }
    }
}

#[async_trait]
impl MobKitStorageProvider for DiskMobKitStorageProvider {
    fn name(&self) -> &'static str {
        "disk"
    }

    async fn open_realm(
        &self,
        ctx: &MobKitRealmOpenContext,
    ) -> Result<MobKitRealmStoreSet, MobKitStorageProviderError> {
        std::fs::create_dir_all(&ctx.state_dir)
            .map_err(|error| Self::open_error("state directory", error))?;
        let layout = &ctx.layout;

        let continuity_path = layout
            .continuity_db()
            .map_err(|error| Self::open_error("continuity", error))?
            .path;
        let (continuity_store, fencing_floor) =
            LocalContinuityStore::open_with_fencing_floor(continuity_path)
                .await
                .map_err(|error| Self::open_error("continuity", error))?;

        let console_path = layout
            .console_db()
            .map_err(|error| Self::open_error("console", error))?
            .path;
        let console_log_store = SqliteConsoleLogStore::open(&console_path)
            .map_err(|error| Self::open_error("console", error))?;

        let metadata_path = layout
            .metadata_db()
            .map_err(|error| Self::open_error("metadata", error))?
            .path;
        let metadata_store = SqliteMetadataStore::open(&metadata_path)
            .map_err(|error| Self::open_error("metadata", error))?;

        let (blob_store, blob_resolution): (Arc<dyn BinaryBlobStore>, DurabilityResolution) =
            if ctx.is_declared_ephemeral("blobs") {
                (
                    Arc::new(ObjectStoreBlobStore::memory()),
                    DurabilityResolution::DeclaredEphemeral,
                )
            } else {
                (
                    Arc::new(
                        ObjectStoreBlobStore::local(layout.blob_root())
                            .map_err(|error| Self::open_error("blobs", error))?,
                    ),
                    DurabilityResolution::Persistent,
                )
            };

        let schedule_store = meerkat::SqliteScheduleStore::open(layout.schedule_db())
            .map_err(|error| Self::open_error("schedule", error))?;

        let agent_memory_root = layout
            .agent_memory_root()
            .map_err(|error| Self::open_error("agent memory", error))?
            .path;
        let agent_memory_provider = SqliteAgentMemoryStore::open(&agent_memory_root)
            .map_err(|error| Self::open_error("agent memory", error))?;

        let set = MobKitRealmStoreSet {
            continuity_store: Arc::new(continuity_store),
            lease_authority: MobKitLeaseAuthority::FencingFloor(fencing_floor),
            // No durable event-log backend ships with the disk bundle; the
            // absence of ingestion is the declared default, overridable per
            // surface (builder `event_log()`, gateway
            // `runtime_options.event_log`).
            event_log_store: None,
            console_log_store: Arc::new(console_log_store),
            metadata_store: Arc::new(metadata_store),
            blob_store,
            agent_memory_provider: Some(Arc::new(agent_memory_provider)),
            schedule_store: Arc::new(schedule_store),
            durability: vec![
                DurabilityDeclaration::durable("continuity", DurabilityResolution::Persistent),
                DurabilityDeclaration::durable(
                    "event_log",
                    DurabilityResolution::DeclaredEphemeral,
                ),
                DurabilityDeclaration::durable("console", DurabilityResolution::Persistent),
                DurabilityDeclaration::durable("metadata", DurabilityResolution::Persistent),
                DurabilityDeclaration::durable("blobs", blob_resolution),
                DurabilityDeclaration::durable("agent_memory", DurabilityResolution::Persistent),
                DurabilityDeclaration::durable("schedule", DurabilityResolution::Persistent),
            ],
        };
        enforce_fail_closed_store_set(&set, ctx)?;
        Ok(set)
    }

    fn meerkat_provider(&self) -> &dyn RealmStorageProvider {
        static DISK: meerkat::storage_provider::DiskStorageProvider =
            meerkat::storage_provider::DiskStorageProvider;
        &DISK
    }

    fn migrator(&self) -> Option<&dyn meerkat_core::StorageMigrator> {
        static MIGRATOR: MobKitStorageMigrator = MobKitStorageMigrator;
        Some(&MIGRATOR)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use meerkat_core::DurabilityClass;

    fn context(dir: &std::path::Path) -> MobKitRealmOpenContext {
        MobKitRealmOpenContext::for_state_dir(dir.join("state"))
    }

    /// The disk provider reproduces today's layout: every durable slot
    /// resolves persistent, the event log stays a declared default, and the
    /// lease authority is the persisted fencing floor.
    #[tokio::test]
    async fn disk_provider_opens_the_canonical_layout_fail_closed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let provider = DiskMobKitStorageProvider;
        assert_eq!(provider.name(), "disk");
        let set = provider
            .open_realm(&context(dir.path()))
            .await
            .expect("disk realm opens");

        assert!(matches!(
            set.lease_authority,
            MobKitLeaseAuthority::FencingFloor(0)
        ));
        assert!(set.event_log_store.is_none());
        assert!(set.agent_memory_provider.is_some());
        assert!(set.blob_store.is_persistent());
        for declaration in &set.durability {
            assert_eq!(declaration.class, DurabilityClass::Durable);
            match declaration.domain.as_str() {
                "event_log" => assert_eq!(
                    declaration.resolution,
                    DurabilityResolution::DeclaredEphemeral
                ),
                _ => assert_eq!(declaration.resolution, DurabilityResolution::Persistent),
            }
        }
        assert!(provider.migrator().is_some());
        assert_eq!(provider.meerkat_provider().name(), "disk");
    }

    /// Declared-ephemeral blobs resolve to the memory backend as an explicit
    /// choice, and the declaration records it.
    #[tokio::test]
    async fn disk_provider_honors_declared_ephemeral_blobs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut ctx = context(dir.path());
        ctx.declared_ephemeral_domains.push("blobs".to_string());
        let set = DiskMobKitStorageProvider
            .open_realm(&ctx)
            .await
            .expect("declared-ephemeral realm opens");
        assert!(!set.blob_store.is_persistent());
        let blob_declaration = set
            .durability
            .iter()
            .find(|declaration| declaration.domain == "blobs")
            .expect("blobs declaration");
        assert_eq!(
            blob_declaration.resolution,
            DurabilityResolution::DeclaredEphemeral
        );
    }

    /// The fail-closed rule: a durable slot resolving non-persistent without
    /// a declaration refuses composition with the typed error.
    #[tokio::test]
    async fn enforce_refuses_undeclared_nonpersistent_durable_slots() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = context(dir.path());
        let mut set = DiskMobKitStorageProvider
            .open_realm(&ctx)
            .await
            .expect("disk realm opens");
        for declaration in &mut set.durability {
            if declaration.domain == "continuity" {
                declaration.resolution = DurabilityResolution::NonPersistent;
            }
        }
        let error = enforce_fail_closed_store_set(&set, &ctx)
            .expect_err("undeclared non-persistent durable slot must refuse");
        assert!(matches!(
            error,
            MobKitStorageProviderError::DurabilityViolation { ref domain } if domain == "continuity"
        ));
        assert!(error.to_string().contains("fail-closed"));

        // The same resolution with the domain declared ephemeral composes.
        let mut declared = ctx.clone();
        declared
            .declared_ephemeral_domains
            .push("continuity".to_string());
        enforce_fail_closed_store_set(&set, &declared)
            .expect("declared ephemeral domain must compose");
    }

    /// Completeness: a provider cannot dodge the fail-closed rule by
    /// omitting a domain's declaration (or declaring it twice).
    #[tokio::test]
    async fn enforce_refuses_omitted_and_duplicate_declarations() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ctx = context(dir.path());
        let set = DiskMobKitStorageProvider
            .open_realm(&ctx)
            .await
            .expect("disk realm opens");

        let mut omitted = set.durability.clone();
        omitted.retain(|declaration| declaration.domain != "metadata");
        let mut incomplete = MobKitRealmStoreSet {
            durability: omitted,
            ..clone_stores(&set)
        };
        let error = enforce_fail_closed_store_set(&incomplete, &ctx)
            .expect_err("an omitted durability declaration must refuse");
        assert!(matches!(
            error,
            MobKitStorageProviderError::DurabilityViolation { ref domain }
                if domain.starts_with("metadata") && domain.contains("0 durability declarations")
        ));

        incomplete.durability = set.durability;
        incomplete.durability.push(DurabilityDeclaration::durable(
            "metadata",
            DurabilityResolution::Persistent,
        ));
        let error = enforce_fail_closed_store_set(&incomplete, &ctx)
            .expect_err("a duplicated durability declaration must refuse");
        assert!(matches!(
            error,
            MobKitStorageProviderError::DurabilityViolation { ref domain }
                if domain.starts_with("metadata") && domain.contains("2 durability declarations")
        ));
    }

    fn clone_stores(set: &MobKitRealmStoreSet) -> MobKitRealmStoreSet {
        MobKitRealmStoreSet {
            continuity_store: Arc::clone(&set.continuity_store),
            lease_authority: set.lease_authority.clone(),
            event_log_store: None,
            console_log_store: Arc::clone(&set.console_log_store),
            metadata_store: Arc::clone(&set.metadata_store),
            blob_store: Arc::clone(&set.blob_store),
            agent_memory_provider: set.agent_memory_provider.clone(),
            schedule_store: Arc::clone(&set.schedule_store),
            durability: set.durability.clone(),
        }
    }
}
