//! Composition-time storage durability resolution (H1/H2 hotfixes of the
//! storage-unification arc).
//!
//! Every durable slot must resolve to a configured backend, an explicitly
//! declared ephemeral choice, or a startup error — never a silent fallback.
//! This module carries the vocabulary for the two hotfixed slots:
//!
//! - **Blobs (H1)**: [`BlobDurability`] records what the blob slot resolved
//!   to; [`BlobStoreResolutionError`] is the fail-closed startup error a
//!   persistent-mode runtime returns instead of the former silent in-memory
//!   fallback.
//! - **Session persistence (H2)**: [`probe_session_store_incremental`]
//!   duplicates the capability probe `PersistentSessionService` runs
//!   privately, so the whole-blob degradation it silently accepts is logged
//!   at startup and visible on the health surfaces.
//!
//! The resolved [`ResolvedStorageSummary`] rides the bootstrap spec onto the
//! runtime and is reported by `mobkit/status` / `mobkit/capabilities`.

use std::path::PathBuf;
use std::sync::Arc;

use meerkat::SessionStore;

/// How the runtime's blob slot was resolved at composition time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobDurability {
    /// Disk-backed object store under the state directory.
    PersistentDisk,
    /// In-memory blobs as an explicitly declared choice — the ephemeral
    /// launch modes, or `UnifiedRuntimeBuilder::ephemeral_blobs(true)`.
    DeclaredEphemeral,
    /// Caller-injected blob store; `persistent` mirrors its
    /// `is_persistent()` report.
    Custom { persistent: bool },
}

impl BlobDurability {
    /// Stable wire spelling used by the health surfaces.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PersistentDisk => "persistent_disk",
            Self::DeclaredEphemeral => "declared_ephemeral",
            Self::Custom { .. } => "custom",
        }
    }

    /// Whether the resolved blob store survives process restart.
    pub fn is_persistent(&self) -> bool {
        match self {
            Self::PersistentDisk => true,
            Self::DeclaredEphemeral => false,
            Self::Custom { persistent } => *persistent,
        }
    }
}

/// Composition-time storage resolution summary, recorded when the runtime's
/// stores are composed and surfaced through `mobkit/status` and
/// `mobkit/capabilities`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedStorageSummary {
    /// What the blob slot resolved to (H1).
    pub blob_durability: BlobDurability,
    /// Whether the session store behind the persistent session service
    /// advertises the incremental-persistence capability (H2). `None` when
    /// the runtime persists no sessions (ephemeral session service).
    pub session_store_incremental: Option<bool>,
}

impl ResolvedStorageSummary {
    /// The `"storage"` object shared by every `mobkit/status` /
    /// `mobkit/capabilities` handler, so the three status shapes stay
    /// field-consistent.
    pub fn status_json(&self) -> serde_json::Value {
        serde_json::json!({
            "blob_durability": self.blob_durability.as_str(),
            "blob_store_persistent": self.blob_durability.is_persistent(),
            "session_store_incremental": self.session_store_incremental,
        })
    }
}

/// Fail-closed blob-slot resolution failure (H1).
#[derive(Debug)]
pub enum BlobStoreResolutionError {
    /// The local blob directory under the persistent state path failed to
    /// open. Formerly a silent in-memory fallback; now a startup error.
    OpenFailed { path: PathBuf, message: String },
    /// Persistent mode resolved a blob store that reports
    /// `!is_persistent()` without the explicit ephemeral-blobs declaration.
    NonPersistentUndeclared,
}

impl std::fmt::Display for BlobStoreResolutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OpenFailed { path, message } => write!(
                f,
                "failed to open persistent binary blob store at {}: {message} \
                 (fix the blob directory, or declare in-memory blobs explicitly \
                 via ephemeral_blobs(true))",
                path.display()
            ),
            Self::NonPersistentUndeclared => write!(
                f,
                "persistent mode resolved a blob store that reports \
                 !is_persistent(); blobs would silently vanish on restart. \
                 Provide a persistent blob store, or declare the ephemeral \
                 choice explicitly via ephemeral_blobs(true)"
            ),
        }
    }
}

impl std::error::Error for BlobStoreResolutionError {}

/// Probe a session store's incremental-persistence capability before handing
/// it to `PersistentSessionService`.
///
/// The service runs the same probe privately and silently degrades to
/// whole-blob persistence (O(session) written per turn) when the capability
/// is absent; this duplicate probe on the same `Arc` makes that degradation
/// loud at startup and feeds the `session_store_incremental` health flag.
/// `store_kind` names the concrete store in the warning (the caller knows
/// which store it composed; the trait object does not).
pub fn probe_session_store_incremental(store: &Arc<dyn SessionStore>, store_kind: &str) -> bool {
    let incremental = Arc::clone(store).as_incremental().is_some();
    if !incremental {
        tracing::warn!(
            session_store = store_kind,
            "session store does not advertise incremental persistence; \
             session persistence degrades to whole-blob saves on every turn \
             (incremental capability absent)"
        );
    }
    incremental
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// Captures formatted tracing output so the tests can assert on the
    /// startup warning text.
    #[derive(Clone, Default)]
    struct CaptureWriter(Arc<std::sync::Mutex<Vec<u8>>>);

    impl CaptureWriter {
        fn contents(&self) -> String {
            String::from_utf8_lossy(
                &self
                    .0
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            )
            .into_owned()
        }
    }

    impl std::io::Write for CaptureWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for CaptureWriter {
        type Writer = CaptureWriter;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    fn probe_with_captured_warnings(
        store: &Arc<dyn SessionStore>,
        store_kind: &str,
    ) -> (bool, String) {
        let writer = CaptureWriter::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(writer.clone())
            .with_max_level(tracing::Level::WARN)
            .finish();
        let incremental = tracing::subscriber::with_default(subscriber, || {
            probe_session_store_incremental(store, store_kind)
        });
        (incremental, writer.contents())
    }

    /// H2: the continuity adapter — the session authority on identity-first
    /// gateways — has no incremental channel; the probe must say so loudly,
    /// naming the store kind and the whole-blob consequence.
    #[tokio::test]
    async fn probe_flags_continuity_adapter_as_whole_blob_and_warns() {
        let dir = tempfile::tempdir().expect("temp dir");
        let (store, _fencing_floor) =
            crate::identity_first::LocalContinuityStore::open_with_fencing_floor(
                dir.path().join("continuity.sqlite"),
            )
            .await
            .expect("open continuity store");
        let adapter: Arc<dyn SessionStore> = Arc::new(
            crate::identity_first::ContinuitySessionStoreAdapter::new(Arc::new(store)),
        );

        let (incremental, warnings) =
            probe_with_captured_warnings(&adapter, "ContinuitySessionStoreAdapter");
        assert!(
            !incremental,
            "the continuity adapter must not advertise incremental persistence"
        );
        assert!(
            warnings.contains("whole-blob"),
            "the startup warning must name the consequence, got: {warnings}"
        );
        assert!(
            warnings.contains("ContinuitySessionStoreAdapter"),
            "the startup warning must name the store kind, got: {warnings}"
        );
    }

    /// H2: an incremental-capable store probes true with no warning.
    #[test]
    fn probe_reports_incremental_sqlite_store_without_warning() {
        let dir = tempfile::tempdir().expect("temp dir");
        let store: Arc<dyn SessionStore> = Arc::new(
            meerkat_store::SqliteSessionStore::open(dir.path().join("sessions.db"))
                .expect("open sqlite session store"),
        );

        let (incremental, warnings) = probe_with_captured_warnings(&store, "SqliteSessionStore");
        assert!(incremental, "SqliteSessionStore advertises as_incremental");
        assert!(
            warnings.is_empty(),
            "no degradation warning expected, got: {warnings}"
        );
    }

    #[test]
    fn blob_durability_wire_spellings_are_stable() {
        assert_eq!(BlobDurability::PersistentDisk.as_str(), "persistent_disk");
        assert_eq!(
            BlobDurability::DeclaredEphemeral.as_str(),
            "declared_ephemeral"
        );
        assert_eq!(
            BlobDurability::Custom { persistent: true }.as_str(),
            "custom"
        );
        assert!(BlobDurability::PersistentDisk.is_persistent());
        assert!(!BlobDurability::DeclaredEphemeral.is_persistent());
        assert!(BlobDurability::Custom { persistent: true }.is_persistent());
        assert!(!BlobDurability::Custom { persistent: false }.is_persistent());
    }

    #[test]
    fn status_json_carries_all_health_fields() {
        let summary = ResolvedStorageSummary {
            blob_durability: BlobDurability::DeclaredEphemeral,
            session_store_incremental: None,
        };
        let json = summary.status_json();
        assert_eq!(json["blob_durability"], "declared_ephemeral");
        assert_eq!(json["blob_store_persistent"], false);
        assert!(json["session_store_incremental"].is_null());

        let summary = ResolvedStorageSummary {
            blob_durability: BlobDurability::PersistentDisk,
            session_store_incremental: Some(true),
        };
        let json = summary.status_json();
        assert_eq!(json["blob_durability"], "persistent_disk");
        assert_eq!(json["blob_store_persistent"], true);
        assert_eq!(json["session_store_incremental"], true);
    }
}
