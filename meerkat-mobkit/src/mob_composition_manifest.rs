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
    /// Whether the launch that created this storage spoke for the durable
    /// composition.
    ///
    /// Defaulted rather than version-gated: manifests written before this field
    /// existed were all written by authoritative launches, because a
    /// non-authoritative launch did not record at all, so `Authoritative` is
    /// the accurate reading of an absent field rather than a fallback.
    #[serde(default)]
    pub created_by_authority: CompositionAuthority,
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
    /// The storage was created by a launch that did not speak for the durable
    /// composition, so no authoritative composition can take effect on it.
    CreatedByRehearsal { manifest: PathBuf, storage: PathBuf },
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
            Self::CreatedByRehearsal { manifest, storage } => write!(
                f,
                "the mob storage at {} was created by a launch that declared it does \
                 not speak for the durable composition (a candidate or certification \
                 pass), recorded at {}, so the composition you supplied can never take \
                 effect on it: a resume cannot apply a new definition, and the event \
                 log's MobCreated definition is the rehearsal one. Create the durable \
                 store from an authoritative launch and run the candidate against a \
                 separate rehearsal path",
                storage.display(),
                manifest.display()
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
    created_by_authority: CompositionAuthority,
) -> Result<(), MobCompositionProvenanceError> {
    let manifest = manifest_path(mob_storage_path);
    let record = MobCompositionManifest {
        manifest_version: MANIFEST_VERSION,
        created_by_mobkit: env!("CARGO_PKG_VERSION").to_string(),
        definition: definition.clone(),
        created_by_authority,
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
    // Before any field comparison. A rehearsal-created store cannot host an
    // authoritative composition at all, so reporting which fields differ would
    // name a fixable-looking cause for something no edit to the definition can
    // fix.
    if !record.created_by_authority.speaks_for_composition() {
        return Err(MobCompositionProvenanceError::CreatedByRehearsal {
            manifest,
            storage: mob_storage_path.to_path_buf(),
        });
    }
    let fields = diverged_definition_fields(&record.definition, supplied);
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

/// Rewrite the manifest after an operator declared a spec update.
///
/// Separate from [`record_on_create`] on purpose, even though the write is the
/// same shape: this one runs against a manifest that ALREADY EXISTS and is being
/// deliberately superseded, which is a different authorization story from
/// recording a fresh composition. Keeping them apart means a create path can
/// never silently overwrite an existing manifest by calling the wrong function.
/// The creating authority is PRESERVED, not reset. A declared update supersedes
/// the composition, never the provenance: if a rehearsal launch created this
/// store, no operator declaration can make the promoted composition take effect
/// on it, because a resume still replays the rehearsal `MobCreated`. Defaulting
/// to authoritative here would launder exactly that store into one that
/// verifies clean.
pub(crate) fn record_declared_update(
    mob_storage_path: &Path,
    declared: &MobDefinition,
) -> Result<(), MobCompositionProvenanceError> {
    let created_by_authority = read_recorded_authority(mob_storage_path);
    record_on_create(mob_storage_path, declared, created_by_authority)
}

/// The authority recorded beside this storage, for callers that must not reset
/// it.
///
/// An unreadable or absent manifest reads as `Authoritative`. That is the safe
/// direction here and only here: this is called on the declared-update path,
/// which already refused unless a manifest was pinned, so the fallback is
/// unreachable in practice, and treating an unreadable manifest as a rehearsal
/// would refuse every future boot of a store whose provenance is merely
/// damaged.
fn read_recorded_authority(mob_storage_path: &Path) -> CompositionAuthority {
    let manifest = manifest_path(mob_storage_path);
    std::fs::read(&manifest)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<MobCompositionManifest>(&bytes).ok())
        .map(|record| record.created_by_authority)
        .unwrap_or_default()
}

/// Which definition fields differ, as dotted PATHS rather than top-level keys.
///
/// Reported at the deepest point where both sides are still objects, so a single
/// changed model pin reads `profiles.security.model` instead of `profiles`.
/// That granularity is the difference between an operator reading the diff and
/// declaring through it unread - HomeCore named this directly, and they are the
/// party who would be clicking past it on every activation.
///
/// Stops descending when either side is a non-object (a list, a scalar, or
/// absent) and reports the path itself: below that point "which element changed"
/// needs a keying rule this does not have, and inventing one would name fields
/// that do not exist.
///
/// Falls back to a whole-definition marker if either side will not serialize, so
/// a serialization failure cannot be read as "no divergence".
pub(crate) fn diverged_definition_fields(
    recorded: &MobDefinition,
    supplied: &MobDefinition,
) -> Vec<String> {
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
    let mut paths = Vec::new();
    collect_diverged_paths("", &recorded_value, &supplied_value, &mut paths);
    if paths.is_empty() && recorded_value != supplied_value {
        // Equality disagreed with the walk: report something rather than
        // claiming agreement.
        return vec!["<whole definition>".to_string()];
    }
    paths.sort_unstable();
    paths.dedup();
    paths
}

fn collect_diverged_paths(
    prefix: &str,
    recorded: &serde_json::Value,
    supplied: &serde_json::Value,
    out: &mut Vec<String>,
) {
    if recorded == supplied {
        return;
    }
    match (recorded.as_object(), supplied.as_object()) {
        (Some(recorded_map), Some(supplied_map)) => {
            let mut keys: Vec<&String> = recorded_map.keys().chain(supplied_map.keys()).collect();
            keys.sort_unstable();
            keys.dedup();
            for key in keys {
                let recorded_child = recorded_map.get(key);
                let supplied_child = supplied_map.get(key);
                if recorded_child == supplied_child {
                    continue;
                }
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                match (recorded_child, supplied_child) {
                    (Some(left), Some(right)) => {
                        collect_diverged_paths(&path, left, right, out);
                    }
                    // Present on one side only: the path IS the difference.
                    _ => out.push(path),
                }
            }
        }
        // Not both objects, so this is as deep as a path can honestly go.
        _ => out.push(if prefix.is_empty() {
            "<whole definition>".to_string()
        } else {
            prefix.to_string()
        }),
    }
}

/// Whether a launch speaks for the durable composition of its mob storage.
///
/// The composition pin assumes one state directory sees one composition. That
/// assumption held for every launch shape I tested and fails for a normal one:
/// a candidate-then-promote pipeline boots a deliberately RESTRICTED composition
/// first, against the same state directory, to certify it. On a fresh store that
/// candidate boot CREATED the pin from the candidate-phase composition, and the
/// promoted boot - carrying the real composition - was then refused on every
/// field the two modes differ in. Measured in production: 929 supervisor
/// respawns of the refused boot before rollback.
///
/// The door cannot reach that state. The refusal happens at BOOT, before any
/// operator ceremony can run, and the composition the operator would declare
/// never existed in the store to begin with.
///
/// So a launch may say it does not speak for the durable composition. Such a
/// launch is not VERIFIED against the pin: it is not claiming to be the durable
/// composition, and a deliberately-restricted certification pass is exactly
/// that claim's opposite.
///
/// It still records provenance when it creates a store, tagged with the
/// authority that created it. Skipping the record instead would leave a store
/// whose composition nobody has spoken for, and the next authoritative resume
/// would then adopt ITS OWN definition as the record while
/// `MobBuilder::for_resume` boots the definition the event log actually holds.
/// That is the certified lie [`record_on_create`] warns about, reached by
/// omission rather than by a bad write.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompositionAuthority {
    /// This launch speaks for the durable composition: it creates the pin when
    /// absent and is verified against it when present. The default, so a launch
    /// that says nothing keeps the protection - the inverse default would mean
    /// forgetting a flag silently loses the pin.
    #[default]
    Authoritative,
    /// This launch does NOT speak for the durable composition - a candidate,
    /// probe, or certification pass whose composition is intentionally not the
    /// one that should be pinned.
    ///
    /// Exempt from VERIFICATION on purpose. Verifying such a launch would wedge
    /// the pipeline in the other direction: the next candidate boot against an
    /// existing pin would be refused for precisely the fields it is meant to
    /// differ in.
    ///
    /// Not exempt from RECORDING. A store this launch created is tagged as
    /// rehearsal-created, and an authoritative resume of it is refused
    /// ([`MobCompositionProvenanceError::CreatedByRehearsal`]) because a resume
    /// structurally cannot apply the promoted composition.
    NonAuthoritative,
}

impl CompositionAuthority {
    /// Whether this launch participates in composition pin semantics at all.
    #[must_use]
    pub fn speaks_for_composition(self) -> bool {
        matches!(self, Self::Authoritative)
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
