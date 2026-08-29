//! Temporary-council store wiring.
//!
//! Councils are a meerkat-mob capability: one `council` tool call seats forked
//! participants, runs bounded exchanges, merges and tears the temporary mob
//! down. `meerkat_mob_mcp::MobMcpState` owns the store and defaults it to an
//! IN-MEMORY implementation, so before this module a MobKit gateway lost every
//! council record on restart - including the unfinished ones that
//! `TemporaryCouncilStore::list_unfinished` exists to recover.
//!
//! MobKit supplies its own store rather than calling
//! `MobMcpState::with_persistent_storage_root`. That helper would also flip
//! forked-participant custody and realm profiles to durable in the same call:
//! three behaviour changes bundled as one, two of them unrelated to councils
//! and each with its own recovery semantics. Supplying one store keeps the
//! change to the thing it claims to change.
//!
//! The path comes from [`crate::MobKitStorageLayout::council_db`], which means
//! the file is a registered [`crate::storage_layout::DatabaseSlot`] and is seen
//! by `storage doctor` and `storage migrate` like every other MobKit database.
//! A durable store outside that enumeration would be a second authority.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use meerkat_mob::store::{SqliteTemporaryCouncilStore, TemporaryCouncilStore};

/// Canonical file name for the temporary-council store.
pub const COUNCIL_STORE_FILE: &str = "council.sqlite3";

/// Open the durable council store for this layout.
///
/// Returns `None` with a warning rather than failing the boot: a gateway that
/// cannot open the council store should still serve, falling back to
/// `MobMcpState`'s in-memory default. Councils are an optional capability and
/// refusing to start over one would trade a degraded feature for an outage.
/// The degrade is LOUD - it logs, and `storage doctor` still reports the slot.
#[must_use]
pub fn open_council_store(path: &Path) -> Option<Arc<dyn TemporaryCouncilStore>> {
    match SqliteTemporaryCouncilStore::open(path) {
        Ok(store) => Some(Arc::new(store)),
        Err(error) => {
            tracing::warn!(
                error = %error,
                path = %path.display(),
                "failed to open the durable council store; councils fall back to \
                 process-bound in-memory storage and will not survive a restart",
            );
            None
        }
    }
}

/// Resolve the council store path for a `store_path`-style root.
///
/// `MobKitStorageLayout::standalone_from_store_path` is the ONE place that
/// interprets the `store_path` escape hatch (a value with an extension names a
/// database and its parent is the state dir; otherwise it IS the state dir).
/// Re-deriving that rule here would create a second path authority, so this
/// goes through the layout even though it only reads `state_dir`.
///
/// `gateway_home` is immaterial to `council_db()`, which reads `state_dir`
/// alone; callers that need a real gateway home must build the layout properly
/// rather than reusing this helper.
#[must_use]
pub fn council_db_for_store_path(store_path: &Path) -> PathBuf {
    crate::MobKitStorageLayout::standalone_from_store_path(store_path, store_path.to_path_buf())
        .council_db()
}
