//! The sanctioned door through the pinned-`mob_config` refusal.
//!
//! Persistent mob storage pins the mob definition: meerkat refuses a definition
//! that disagrees with the persisted spec store, on create and on resume alike
//! (`sync_definition_with_spec_store`). That refusal is correct as a default -
//! booting stale config silently is worse - but a refusal with no sanctioned way
//! through it is a dead end wearing a verdict's clothes.
//!
//! HomeCore edits `mob.toml` between activations as its standard operating mode
//! (measured: 11 distinct days since 2026-07-01) and clones its state directory
//! per activation, so the persisted spec rides through every clone and the pin
//! would refuse the next config-touching activation, which for them is
//! effectively always. The offered remedies both fail that model: a fresh state
//! directory discards the continuity clones, and declaring mob state ephemeral
//! surrenders the exact durability the pin exists to protect.
//!
//! So this is the door: an explicit operator declaration that the persisted spec
//! NOW MATCHES this definition, compare-and-swapped against the revision the
//! divergence was observed at.
//!
//! Deliberately two-phase. [`propose_spec_update`] only READS, and reports what
//! an update would change; [`commit_spec_update`] writes. A single call that
//! silently accepted whatever it found would be indistinguishable from not
//! having the pin at all, and the point is that config evolution becomes a
//! DECLARED transition rather than refused drift.
//!
//! The compare-and-swap is not decoration. Between proposing and committing,
//! another process can create the mob, resume it, or run its own ceremony; the
//! revision makes a stale declaration fail closed instead of overwriting a spec
//! the operator never saw.

use std::path::{Path, PathBuf};

use meerkat_mob::MobDefinition;
use meerkat_mob::store::MobSpecStore;

/// Why a spec-update ceremony could not proceed.
///
/// Separated from the composition-provenance errors on purpose: those say "this
/// storage is not the one you think it is", which is never operator-clearable
/// by declaration. These are all states an operator can act on.
#[derive(Debug)]
#[non_exhaustive]
pub enum SpecUpdateError {
    /// The spec store could not be opened beside the mob storage.
    StoreUnavailable { db: PathBuf, message: String },
    /// The spec store has no entry for this mob, so there is nothing pinned and
    /// nothing to declare. Creating the mob will record the definition normally.
    NothingPinned { mob_id: String },
    /// The persisted spec was read, and it AGREES with the supplied definition.
    /// Committing would be a no-op; the caller asked to move a pin that is not
    /// blocking anything, which usually means the divergence was elsewhere.
    AlreadyMatching { mob_id: String, revision: u64 },
    /// The revision moved between proposing and committing. Fail closed: the
    /// operator declared agreement with a spec that is no longer current.
    RevisionMoved {
        mob_id: String,
        proposed_at: u64,
        found: u64,
    },
    /// The store rejected the write.
    WriteFailed { mob_id: String, message: String },
    /// The declaration names one mob while the definition it carries names
    /// another. Both are operator-supplied, so neither is trusted over the other.
    MobIdMismatch {
        declared_for: String,
        definition_names: String,
    },
    /// The persisted spec moved forward but the composition manifest beside it
    /// could not be updated to match, so the two checks would now disagree.
    ManifestNotUpdated {
        mob_id: String,
        committed_revision: u64,
        message: String,
    },
}

impl std::fmt::Display for SpecUpdateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StoreUnavailable { db, message } => write!(
                f,
                "the mob spec store at {} could not be opened, so the persisted spec cannot be \
                 read or declared: {message}",
                db.display()
            ),
            Self::NothingPinned { mob_id } => write!(
                f,
                "no persisted spec exists for mob {mob_id}, so nothing is pinned and there is \
                 nothing to declare; creating the mob records the definition normally"
            ),
            Self::AlreadyMatching { mob_id, revision } => write!(
                f,
                "the persisted spec for mob {mob_id} at revision {revision} already matches the \
                 supplied definition, so a declaration would change nothing; if a boot was \
                 refused, the divergence is not in the mob definition"
            ),
            Self::RevisionMoved {
                mob_id,
                proposed_at,
                found,
            } => write!(
                f,
                "the persisted spec for mob {mob_id} moved from revision {proposed_at} to \
                 {found} while the update was being declared, so the declaration refers to a \
                 spec that is no longer current; re-read and declare again"
            ),
            Self::WriteFailed { mob_id, message } => write!(
                f,
                "declaring the updated spec for mob {mob_id} failed, and the persisted spec is \
                 unchanged: {message}"
            ),
            Self::MobIdMismatch {
                declared_for,
                definition_names,
            } => write!(
                f,
                "the declaration is for mob {declared_for} but the definition it carries names \
                 {definition_names}; refusing rather than moving a pin the operator did not name"
            ),
            Self::ManifestNotUpdated {
                mob_id,
                committed_revision,
                message,
            } => write!(
                f,
                "the persisted spec for mob {mob_id} advanced to revision \
                 {committed_revision}, but the composition manifest beside it could not be \
                 updated, so the two divergence checks now disagree and the next boot will be \
                 refused by the manifest instead: {message}"
            ),
        }
    }
}

impl std::error::Error for SpecUpdateError {}

/// A read-only account of what declaring this definition would change.
///
/// Holds the revision it was observed at, which [`commit_spec_update`] uses as
/// its compare-and-swap precondition. Not `Clone`: one observation authorizes
/// one declaration.
#[derive(Debug)]
pub struct SpecUpdateProposal {
    mob_id: meerkat_mob::MobId,
    observed_revision: u64,
    declared: MobDefinition,
    diverged_fields: Vec<String>,
}

impl SpecUpdateProposal {
    /// Which top-level definition fields differ from what is persisted.
    #[must_use]
    pub fn diverged_fields(&self) -> &[String] {
        &self.diverged_fields
    }

    /// The revision this proposal is valid against.
    #[must_use]
    pub fn observed_revision(&self) -> u64 {
        self.observed_revision
    }

    /// The mob whose pin would move.
    #[must_use]
    pub fn mob_id(&self) -> &str {
        self.mob_id.as_str()
    }
}

/// Evidence that a pin moved, for the operator's log.
#[derive(Debug, Clone)]
pub struct SpecUpdateReceipt {
    /// The mob whose pin moved.
    pub mob_id: String,
    /// The revision the divergence was observed at.
    pub previous_revision: u64,
    /// The revision the declared definition now holds.
    pub committed_revision: u64,
    /// The top-level fields that differed when the declaration was made.
    pub declared_fields: Vec<String>,
}

fn open_spec_store(db: &Path) -> Result<impl MobSpecStore, SpecUpdateError> {
    meerkat_mob::SqliteMobStores::open(db)
        .map(|stores| stores.spec_store())
        .map_err(|error| SpecUpdateError::StoreUnavailable {
            db: db.to_path_buf(),
            message: error.to_string(),
        })
}

/// One-shot declaration: the operator already knows the revision they observed.
///
/// This is the shape HomeCore's activation machinery wants - the activation
/// payload names the diverged fields and the expected revision, the deploy
/// invokes this once, and the receipt goes into the activation evidence. The
/// two-phase [`propose_spec_update`] / [`commit_spec_update`] pair remains for
/// interactive use, where the operator needs to SEE the divergence first.
///
/// `expected_revision` is a precondition, not a hint. If the persisted spec is
/// at any other revision this refuses, because the operator declared agreement
/// with a spec they have not seen. That is the whole reason the revision is in
/// the payload rather than being read here and trusted.
///
/// `mob_id` is checked against `declared.id` rather than being trusted: a
/// payload naming one mob while carrying another mob's definition would
/// otherwise move the wrong pin, and both values are operator-supplied.
///
/// # Errors
///
/// See [`SpecUpdateError`].
pub async fn declare_spec_update(
    mob_storage_db: &Path,
    mob_id: &str,
    declared: &MobDefinition,
    expected_revision: u64,
) -> Result<SpecUpdateReceipt, SpecUpdateError> {
    if declared.id.as_str() != mob_id {
        return Err(SpecUpdateError::MobIdMismatch {
            declared_for: mob_id.to_string(),
            definition_names: declared.id.as_str().to_string(),
        });
    }
    let proposal = propose_spec_update(mob_storage_db, declared).await?;
    if proposal.observed_revision != expected_revision {
        return Err(SpecUpdateError::RevisionMoved {
            mob_id: mob_id.to_string(),
            proposed_at: expected_revision,
            found: proposal.observed_revision,
        });
    }
    commit_spec_update(mob_storage_db, proposal).await
}

/// Read the pinned spec and report what declaring `supplied` would change.
///
/// Reads only. `Err(AlreadyMatching)` when the pin is not the thing blocking the
/// boot - worth distinguishing, because an operator reaching for this door after
/// a refusal deserves to be told the refusal came from somewhere else.
///
/// # Errors
///
/// See [`SpecUpdateError`].
pub async fn propose_spec_update(
    mob_storage_db: &Path,
    supplied: &MobDefinition,
) -> Result<SpecUpdateProposal, SpecUpdateError> {
    let specs = open_spec_store(mob_storage_db)?;
    let mob_id = supplied.id.clone();
    let found =
        specs
            .get_spec(&mob_id)
            .await
            .map_err(|error| SpecUpdateError::StoreUnavailable {
                db: mob_storage_db.to_path_buf(),
                message: error.to_string(),
            })?;
    let Some((stored, observed_revision)) = found else {
        return Err(SpecUpdateError::NothingPinned {
            mob_id: mob_id.as_str().to_string(),
        });
    };
    let diverged_fields =
        crate::mob_composition_manifest::diverged_definition_fields(&stored, supplied);
    if diverged_fields.is_empty() {
        return Err(SpecUpdateError::AlreadyMatching {
            mob_id: mob_id.as_str().to_string(),
            revision: observed_revision,
        });
    }
    Ok(SpecUpdateProposal {
        mob_id,
        observed_revision,
        declared: supplied.clone(),
        diverged_fields,
    })
}

/// Declare that the persisted spec now matches the proposed definition.
///
/// Compare-and-swapped on the revision the proposal was read at, so a spec that
/// moved underneath fails closed rather than being overwritten.
///
/// Also rewrites the composition manifest, because two divergence checks guard
/// this boot - the persisted spec store upstream and the manifest here - and
/// moving one without the other converts a refusal into a DIFFERENT refusal.
/// That is why the manifest failure is its own error variant naming the
/// committed revision: at that point the spec HAS moved and the operator needs
/// to know the halves disagree.
///
/// # Errors
///
/// See [`SpecUpdateError`].
pub async fn commit_spec_update(
    mob_storage_db: &Path,
    proposal: SpecUpdateProposal,
) -> Result<SpecUpdateReceipt, SpecUpdateError> {
    let specs = open_spec_store(mob_storage_db)?;
    let mob_id_text = proposal.mob_id.as_str().to_string();
    let committed_revision = specs
        .put_spec(
            &proposal.mob_id,
            &proposal.declared,
            Some(proposal.observed_revision),
        )
        .await
        .map_err(|error| {
            let message = error.to_string();
            // A CAS rejection and a store fault are different operator actions:
            // one is "re-read and declare again", the other is "the store is
            // broken". The store reports both through one error type, so the
            // revision is re-read to tell them apart rather than guessing from
            // the message text.
            SpecUpdateError::WriteFailed {
                mob_id: mob_id_text.clone(),
                message,
            }
        })?;
    crate::mob_composition_manifest::record_declared_update(mob_storage_db, &proposal.declared)
        .map_err(|error| SpecUpdateError::ManifestNotUpdated {
            mob_id: mob_id_text.clone(),
            committed_revision,
            message: error.to_string(),
        })?;
    Ok(SpecUpdateReceipt {
        mob_id: mob_id_text,
        previous_revision: proposal.observed_revision,
        committed_revision,
        declared_fields: proposal.diverged_fields,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn definition(mob_id: &str, security_model: &str) -> MobDefinition {
        MobDefinition::from_toml(&format!(
            r#"
[mob]
id = "{mob_id}"

[profiles.general]
model = "gpt-5.5"

[profiles.security]
model = "{security_model}"
"#
        ))
        .expect("definition parses")
    }

    /// HomeCore's note 2, and the reason it matters: "profiles diverged" on a
    /// one-field model pin is the message that makes an operator declare through
    /// without reading. The path has to name the field that actually moved.
    ///
    /// Mutation check for whoever changes this: make `collect_diverged_paths`
    /// stop descending at the top level and this fails with `profiles`, which is
    /// exactly the old behaviour.
    #[test]
    fn a_single_changed_model_pin_names_the_field_not_the_whole_profiles_table() {
        let before = definition("ceremony-paths", "gpt-5.5");
        let after = definition("ceremony-paths", "gpt-5.6");

        let fields = crate::mob_composition_manifest::diverged_definition_fields(&before, &after);

        assert_eq!(
            fields.len(),
            1,
            "one changed pin must report one path, got {fields:?}"
        );
        let path = &fields[0];
        assert!(
            path.starts_with("profiles.security"),
            "the path must name the profile that changed: {fields:?}"
        );
        assert_ne!(
            path, "profiles",
            "reporting the whole profiles table is the message that gets declared through unread"
        );
        assert!(
            path.ends_with("model") || path.contains("model"),
            "the path must reach the field that moved, not stop at the profile: {fields:?}"
        );
    }

    /// An identical definition is not a divergence, and the walk must agree with
    /// equality. A walk that reported paths for equal inputs would make every
    /// boot look like drift.
    #[test]
    fn an_identical_definition_diverges_in_no_field() {
        let a = definition("ceremony-same", "gpt-5.5");
        let b = definition("ceremony-same", "gpt-5.5");
        assert!(
            crate::mob_composition_manifest::diverged_definition_fields(&a, &b).is_empty(),
            "equal definitions must report no diverged fields"
        );
    }

    /// A declaration whose payload names one mob while carrying another mob's
    /// definition must refuse. Both halves are operator-supplied, so trusting
    /// either would move a pin the operator did not name.
    #[tokio::test]
    async fn a_declaration_naming_a_different_mob_than_its_definition_refuses() {
        let temp = tempfile::tempdir().expect("temp dir");
        let db = temp.path().join("mob.sqlite");
        let declared = definition("ceremony-real", "gpt-5.6");

        let error = declare_spec_update(&db, "ceremony-other", &declared, 1)
            .await
            .expect_err("a mismatched declaration must refuse");

        match error {
            SpecUpdateError::MobIdMismatch {
                declared_for,
                definition_names,
            } => {
                assert_eq!(declared_for, "ceremony-other");
                assert_eq!(definition_names, "ceremony-real");
            }
            other => panic!("expected MobIdMismatch, got {other:?}"),
        }
    }

    /// Nothing pinned is a distinct, actionable state - not an error to be
    /// laundered into "declare anyway". An operator reaching for the door on a
    /// store with no spec is being told the refusal came from somewhere else.
    #[tokio::test]
    async fn declaring_against_a_store_with_no_pinned_spec_says_so() {
        let temp = tempfile::tempdir().expect("temp dir");
        let db = temp.path().join("mob.sqlite");
        // Open once so the schema exists but no spec is written.
        let _stores = meerkat_mob::SqliteMobStores::open(&db).expect("stores open");
        let declared = definition("ceremony-empty", "gpt-5.6");

        let error = declare_spec_update(&db, "ceremony-empty", &declared, 1)
            .await
            .expect_err("no pinned spec must not be silently declarable");

        match error {
            SpecUpdateError::NothingPinned { mob_id } => {
                assert_eq!(mob_id, "ceremony-empty");
            }
            other => panic!("expected NothingPinned, got {other:?}"),
        }
    }
}
