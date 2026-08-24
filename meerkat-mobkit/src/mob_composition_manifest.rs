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
    /// A storage claiming to be ephemeral arrived already holding events.
    ///
    /// A genuinely in-memory storage is constructed fresh and is therefore
    /// empty at bootstrap. A non-empty event log under a
    /// [`MobStorageProvenance::DeclaredEphemeral`] claim means the claim is
    /// unproven: something composed durable or pre-seeded storage and did not
    /// declare it. Without this check `DeclaredEphemeral` would be a third
    /// silent state - asserted rather than evidenced - and such a storage
    /// would resume with no composition verification at all.
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
                "a mob storage supplied to bootstrap already holds events but was not \
                 declared persistent, so this build cannot verify that resuming it would \
                 boot the composition supplied (an in-memory storage is empty at \
                 bootstrap, so a non-empty one is either durable or pre-seeded); compose \
                 it through mob_composition_manifest::persistent_mob_storage and pass the \
                 returned provenance to MobBootstrapSpec::with_mob_storage_provenance, or \
                 supply an empty storage"
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
/// Called on the create branch only. Overwrites any stale record, since an
/// empty event log means no prior mob survives on this path.
pub fn record_on_create(
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

/// Whether a composed mob storage carries a composition that outlives the
/// process, and if so where.
///
/// This is an explicit two-state declaration rather than an `Option<PathBuf>`
/// so that "ephemeral by declaration" and "persistent at a path" are distinct
/// states with no third, silent one. An undeclared persistent path would be
/// exactly the silence this repair exists to remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MobStorageProvenance {
    /// In-memory by declaration: nothing survives the process, so there is no
    /// cross-restart composition to judge.
    DeclaredEphemeral,
    /// Persistent at this path: provenance is recorded on create and verified
    /// before any resume.
    Persistent { path: PathBuf },
}

impl MobStorageProvenance {
    /// The path when this storage is persistent.
    pub fn persistent_path(&self) -> Option<&Path> {
        match self {
            Self::DeclaredEphemeral => None,
            Self::Persistent { path } => Some(path),
        }
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
    Ok((storage, MobStorageProvenance::Persistent { path }))
}
