//! MobKit-owned composition provenance for a persistent mob storage path.
//!
//! Mob storage is event-sourced: a non-empty event log already carries the
//! `MobCreated` definition, and [`meerkat_mob::MobBuilder::for_resume`] takes
//! only the storage - there is structurally no way to pass a fresh definition
//! into a resume. That is correct for the event log's own authority, but it
//! means a bare resume branch would silently boot the definition recorded at
//! first create even after the operator edited their config. The boot would
//! look healthy: right member count, right phases, wrong composition.
//!
//! This module records what composition the storage path was created for, so a
//! resume can refuse before the mob actuates rather than after it misbehaves.
//! It records *provenance*, not a second semantic definition: the event log
//! stays the sole authority for what the mob is, and this file only answers
//! "was this storage created for the composition I was just handed?".
//!
//! Comparison is structural rather than digest-based, deliberately. A digest
//! can only report "different", while the recorded definition can name which
//! fields diverged - and a digest over serialized bytes would mismatch
//! spuriously if serialization ordering ever changed, turning "fail loud on
//! divergence" into "fail loud on upgrade".

use std::path::{Path, PathBuf};

use meerkat_mob::MobDefinition;
use serde::{Deserialize, Serialize};

/// Current manifest schema version.
pub const MANIFEST_VERSION: u32 = 1;

/// Provenance record written beside a persistent mob storage path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobCompositionManifest {
    /// Schema version of this file.
    pub manifest_version: u32,
    /// MobKit version that created the storage path.
    pub created_by_mobkit: String,
    /// The composition the storage path was created for.
    pub definition: MobDefinition,
}

/// Where the manifest lives for a given mob storage path: beside it, with the
/// storage file name preserved so two mobs on one directory cannot collide.
pub fn manifest_path(mob_storage_path: &Path) -> PathBuf {
    let mut file_name = mob_storage_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mob".to_string());
    file_name.push_str(".composition.json");
    mob_storage_path.with_file_name(file_name)
}

/// Fail-closed composition provenance failure. Every variant is an init-time
/// refusal raised *before* the mob actuates.
#[derive(Debug)]
pub enum MobCompositionProvenanceError {
    /// Non-empty storage with no provenance record beside it.
    Missing { manifest: PathBuf, storage: PathBuf },
    /// The provenance record exists but could not be read.
    Unreadable { manifest: PathBuf, message: String },
    /// The provenance record was read but is not a manifest.
    Malformed { manifest: PathBuf, message: String },
    /// The provenance record is a manifest of a schema this build cannot judge.
    UnsupportedVersion {
        manifest: PathBuf,
        found: u32,
        supported: u32,
    },
    /// The supplied composition differs from the one the storage was created
    /// for.
    Divergent {
        manifest: PathBuf,
        fields: Vec<String>,
    },
    /// The provenance record could not be written at create time.
    NotRecorded { manifest: PathBuf, message: String },
    /// A storage arrived already holding events with nothing declared about
    /// what it is.
    ///
    /// `MobBootstrapSpec::new` takes arbitrary storage, so an undeclared
    /// non-empty log could be a durable database an external embedder opened
    /// directly. Resuming it would skip composition verification entirely, and
    /// no default claim can be safely inferred here: the constructor cannot
    /// know what the caller handed it.
    UnprovenStorage,
}

impl std::fmt::Display for MobCompositionProvenanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { manifest, storage } => write!(
                f,
                "the mob storage at {} already holds events but has no composition \
                 provenance at {}, so this build cannot tell whether resuming it would \
                 boot the composition you supplied or an older one (refusing before the \
                 mob actuates; if the storage is known-good and matches your current \
                 config, remove the storage to recreate it, or restore the manifest \
                 written beside it)",
                storage.display(),
                manifest.display()
            ),
            Self::Unreadable { manifest, message } => write!(
                f,
                "failed to read the mob composition provenance at {}: {} (resuming \
                 blind could boot a stale composition that looks healthy; fix the file \
                 permissions or restore the file)",
                manifest.display(),
                message
            ),
            Self::Malformed { manifest, message } => write!(
                f,
                "the mob composition provenance at {} is not a readable manifest: {} \
                 (resuming blind could boot a stale composition that looks healthy; \
                 restore the file, or remove the mob storage to recreate it)",
                manifest.display(),
                message
            ),
            Self::UnsupportedVersion {
                manifest,
                found,
                supported,
            } => write!(
                f,
                "the mob composition provenance at {} is schema version {found}, but \
                 this build understands version {supported} and cannot judge whether \
                 the stored composition matches the one supplied (upgrade the gateway \
                 to a build that understands version {found}, or remove the mob storage \
                 to recreate it)",
                manifest.display()
            ),
            Self::Divergent { manifest, fields } => write!(
                f,
                "the supplied mob definition diverges from the composition this storage \
                 was created for, in: {} (a resume cannot apply a new definition - the \
                 event log's MobCreated definition is authoritative - so booting would \
                 silently run the composition recorded at {}; revert the change, or \
                 create a new mob storage path for the new composition)",
                fields.join(", "),
                manifest.display()
            ),
            Self::UnprovenStorage => write!(
                f,
                "a mob storage supplied to bootstrap already holds events but nothing \
                 was declared about what that storage is, so this build cannot verify \
                 that resuming it would boot the composition supplied rather than an \
                 older one; if it is durable, compose it through \
                 mob_composition_manifest::persistent_mob_storage and pass the returned \
                 provenance to MobBootstrapSpec::with_mob_storage_provenance, and if it \
                 is in-process only, declare that with \
                 MobBootstrapSpec::with_declared_ephemeral_mob_storage"
            ),
            Self::NotRecorded { manifest, message } => write!(
                f,
                "failed to record mob composition provenance at {}: {} (without it the \
                 next restart cannot prove the stored composition matches your config, \
                 so refusing now rather than leaving an unjudgeable storage path behind)",
                manifest.display(),
                message
            ),
        }
    }
}

impl std::error::Error for MobCompositionProvenanceError {}

/// Record the composition a fresh persistent storage path was created for.
///
/// Crate-internal, and called on the create branch ONLY. Exposing it would let
/// a caller stamp a new composition onto a non-empty store, which makes
/// [`verify_before_resume`] pass while `MobBuilder::for_resume` still boots the
/// definition the event log actually holds - a certified lie.
///
/// Called on the create branch only. Overwrites any stale record, since an
/// empty event log means no prior mob survives on this path.
pub(crate) fn record_on_create(
    mob_storage_path: &Path,
    definition: &MobDefinition,
) -> Result<(), MobCompositionProvenanceError> {
    let manifest = manifest_path(mob_storage_path);
    let record = MobCompositionManifest {
        manifest_version: MANIFEST_VERSION,
        created_by_mobkit: env!("CARGO_PKG_VERSION").to_string(),
        definition: definition.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&record).map_err(|err| {
        MobCompositionProvenanceError::NotRecorded {
            manifest: manifest.clone(),
            message: err.to_string(),
        }
    })?;
    if let Some(parent) = manifest.parent() {
        std::fs::create_dir_all(parent).map_err(|err| {
            MobCompositionProvenanceError::NotRecorded {
                manifest: manifest.clone(),
                message: err.to_string(),
            }
        })?;
    }
    std::fs::write(&manifest, bytes).map_err(|err| MobCompositionProvenanceError::NotRecorded {
        manifest: manifest.clone(),
        message: err.to_string(),
    })
}

/// Verify a non-empty storage path was created for the supplied composition.
///
/// Called on the resume branch, before the builder is constructed, so every
/// refusal lands before the mob can actuate.
pub fn verify_before_resume(
    mob_storage_path: &Path,
    supplied: &MobDefinition,
) -> Result<(), MobCompositionProvenanceError> {
    let manifest = manifest_path(mob_storage_path);
    let bytes = match std::fs::read(&manifest) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(MobCompositionProvenanceError::Missing {
                manifest,
                storage: mob_storage_path.to_path_buf(),
            });
        }
        Err(err) => {
            return Err(MobCompositionProvenanceError::Unreadable {
                manifest,
                message: err.to_string(),
            });
        }
    };
    // Read the version before the whole manifest: a future schema must produce
    // UnsupportedVersion, not Malformed, or the operator is told to restore a
    // file that is in fact newer than this build.
    let version = serde_json::from_slice::<ManifestVersionProbe>(&bytes)
        .map(|probe| probe.manifest_version)
        .map_err(|err| MobCompositionProvenanceError::Malformed {
            manifest: manifest.clone(),
            message: err.to_string(),
        })?;
    if version != MANIFEST_VERSION {
        return Err(MobCompositionProvenanceError::UnsupportedVersion {
            manifest,
            found: version,
            supported: MANIFEST_VERSION,
        });
    }
    let record = serde_json::from_slice::<MobCompositionManifest>(&bytes).map_err(|err| {
        MobCompositionProvenanceError::Malformed {
            manifest: manifest.clone(),
            message: err.to_string(),
        }
    })?;
    let fields = diverged_fields(&record.definition, supplied);
    if fields.is_empty() {
        Ok(())
    } else {
        Err(MobCompositionProvenanceError::Divergent { manifest, fields })
    }
}

/// Version-only view, so an unreadable *body* and an unjudgeable *schema* stay
/// distinguishable.
#[derive(Deserialize)]
struct ManifestVersionProbe {
    manifest_version: u32,
}

/// Top-level definition fields that differ, for an actionable refusal.
///
/// Falls back to a whole-definition marker if either side will not serialize,
/// so a serialization failure cannot be read as "no divergence".
fn diverged_fields(recorded: &MobDefinition, supplied: &MobDefinition) -> Vec<String> {
    let (Ok(recorded_value), Ok(supplied_value)) = (
        serde_json::to_value(recorded),
        serde_json::to_value(supplied),
    ) else {
        return if recorded == supplied {
            Vec::new()
        } else {
            vec!["<whole definition>".to_string()]
        };
    };
    match (recorded_value.as_object(), supplied_value.as_object()) {
        (Some(recorded_map), Some(supplied_map)) => {
            let mut keys: Vec<&String> = recorded_map.keys().chain(supplied_map.keys()).collect();
            keys.sort_unstable();
            keys.dedup();
            keys.into_iter()
                .filter(|key| recorded_map.get(*key) != supplied_map.get(*key))
                .cloned()
                .collect()
        }
        _ if recorded_value == supplied_value => Vec::new(),
        _ => vec!["<whole definition>".to_string()],
    }
}

/// What a composed mob storage is, as declared by whoever composed it.
///
/// `MobStorage` does not report its own durability, so this cannot be derived
/// and has to be declared. The default is "unspecified" rather than an
/// ephemeral claim deliberately: `MobBootstrapSpec::new` accepts arbitrary
/// storage, so claiming ephemerality would assert a fact the constructor
/// cannot know, and an external embedder passing durable storage would
/// silently inherit that claim.
///
/// Opaque on purpose. The persistent form is only constructible inside this
/// crate, by [`persistent_mob_storage`], which returns it paired with the
/// storage it describes. A public `Persistent { path }` would let a caller
/// pair storage A with storage B's manifest path and have bootstrap verify the
/// wrong composition, which is exactly the guarantee the paired constructor
/// exists to provide.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MobStorageProvenance(Provenance);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Provenance {
    /// Nothing was declared. Safe while the event log is empty (a create needs
    /// no provenance), and fails closed the moment a non-empty log would be
    /// resumed unverified.
    #[default]
    Unspecified,
    /// In-memory by declaration: nothing survives the process, so there is no
    /// cross-restart composition to judge. A pre-seeded in-memory storage may
    /// resume without composition verification, because the composition it
    /// would be checked against was built in this same process.
    DeclaredEphemeral,
    /// Persistent at this path: provenance is recorded on create and verified
    /// before any resume.
    Persistent { path: PathBuf },
}

impl MobStorageProvenance {
    /// Declare that the storage lives only for this process.
    ///
    /// This is a claim the caller is making about storage this crate cannot
    /// inspect. It is the deliberate escape for in-process callers and for
    /// launches that would rather keep an editable definition than durable mob
    /// state; it is never a default.
    pub fn declared_ephemeral() -> Self {
        Self(Provenance::DeclaredEphemeral)
    }

    /// Crate-internal: only [`persistent_mob_storage`] may say a storage is
    /// persistent, and only about the path it just opened.
    fn persistent(path: PathBuf) -> Self {
        Self(Provenance::Persistent { path })
    }

    /// The path when this storage is persistent.
    pub fn persistent_path(&self) -> Option<&Path> {
        match &self.0 {
            Provenance::Unspecified | Provenance::DeclaredEphemeral => None,
            Provenance::Persistent { path } => Some(path),
        }
    }

    /// Whether a non-empty event log may be resumed under this declaration.
    ///
    /// Only the unspecified form is refused: an undeclared storage could be
    /// durable, and resuming it would skip composition verification entirely.
    pub fn permits_unverified_resume(&self) -> bool {
        !matches!(self.0, Provenance::Unspecified)
    }
}

/// The one way MobKit composes persistent mob storage.
///
/// Returns the storage paired with its provenance, so a persistent mob storage
/// cannot be composed anywhere in MobKit without the record that makes it
/// judgeable on the next restart. Three launch paths previously composed mob
/// storage independently and drifted; pairing the two facts in one return
/// value is what keeps them from drifting again.
pub fn persistent_mob_storage(
    path: PathBuf,
) -> Result<(meerkat_mob::MobStorage, MobStorageProvenance), meerkat_mob::MobError> {
    let storage = meerkat_mob::MobStorage::persistent(&path)?;
    Ok((storage, MobStorageProvenance::persistent(path)))
}
