//! Shared admission control for WorkGraph attention-binding mutations.
//!
//! Upstream (meerkat 0.7.23) happily gives one member a second Active
//! attention binding, after which every scoped turn of that member is a hard
//! `MultipleActiveBindings` error until an operator intervenes — a bricked
//! member. MobKit therefore refuses to ADMIT the second binding, and this
//! module is the single place that refusal lives: the occupancy check
//! (with session↔identity alias resolution through the mob roster), the
//! in-process gate serializing every check-then-act window, and — for
//! SQLite-backed stores that two processes may share — a cross-process
//! sidecar lock.
//!
//! One [`WorkGraphAdmission`] exists per [`MobRuntime`](crate::MobRuntime).
//! Every surface that can mint an attention binding must go through it:
//! - the `mobkit/workgraph/*` RPC arms (unified stdin + console) for
//!   `goal/create`, `attention/resume` and `attention/reassign`;
//! - the AGENT TOOL plane: `ScopePinnedWorkGraphTools` intercepts
//!   `workgraph_attention_reassign` through a late-bound
//!   [`WorkGraphAdmissionSlot`] that [`MobRuntime::bootstrap`] fills (the
//!   tool wrapper is constructed before the mob — and thus the roster —
//!   exists). An unfilled slot (non-mob embedder) forwards unguarded, as
//!   before.
//!
//! A surface that checked without holding the gate would race the others
//! past the check; a surface that skipped the check (the round-3 tool-plane
//! hole) would both brick the member and invert authority — an agent doing
//! what an ABAC-granted operator is refused.
//!
//! # Target spelling: normalize at write, alias at read (round-4 Q2)
//!
//! The roster is PROCESS-LOCAL, but the SQLite store is documented as
//! shareable by two processes (gateway + library-mode runtime on one state
//! dir). A guard that needed the roster to equate a session-form row with an
//! identity-form check would be alias-blind in the process that doesn't know
//! the member — and in-process while a member is mid-respawn (absent from
//! the roster). So mobkit normalizes at WRITE instead: every mutation that
//! points a binding at a target (`goal/create` and `attention/reassign` on
//! the RPC arms, `workgraph_attention_reassign` on the tool plane) first
//! lowers a session target that resolves to a roster member to its OWNER
//! form (`mob/<mob>/agent/<identity>`) via
//! [`WorkGraphAdmission::lower_member_session_target`]. Mobkit-created
//! bindings are therefore owner-form whenever a member is involved, and the
//! occupancy check's roster-FREE layer — primary owner-key equality, which
//! for non-member sessions is raw-session-id equality — refuses duplicates
//! without consulting any roster. The roster alias resolution in
//! [`attention_target_alias_keys`] remains as an EXTRA layer for legacy or
//! CLI-created session-form rows: bindings written by the meerkat CLI
//! directly on a shared store bypass this normalization and can still alias;
//! the guard catches those only when the local roster knows the member.
//!
//! # Occupancy-scan bounds
//!
//! The occupancy check queries `list_attention` once per occupying status
//! (Active, Paused) with the service's realm and namespace pinned, so
//! upstream filters before returning rather than handing back every
//! permanently-accumulating Superseded/Stopped row. The upstream store-level
//! SELECT itself has no WHERE clause (meerkat 0.7.23,
//! meerkat-workgraph/src/store.rs `list_sqlite_attention` filters in Rust
//! after a full scan) — that bound is upstream's; an upstream ask is filed
//! separately.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use meerkat::{
    AttentionBindingRequest, AttentionListRequest, GoalAttentionTarget, WorkAttentionBinding,
    WorkAttentionBindingId, WorkAttentionStatus, WorkAttentionTarget, WorkGraphError,
    WorkGraphService, WorkNamespace, WorkOwnerKey, WorkOwnerKind,
};

/// File name of the cross-process admission lock database, created beside
/// [`WORKGRAPH_STORE_FILE`](crate::workgraph_wiring::WORKGRAPH_STORE_FILE).
/// Deliberately a SEPARATE file: holding a write transaction on the real
/// store across the check-then-mutate window would deadlock against the
/// service's own writes mid-admission.
pub const WORKGRAPH_ADMISSION_SIDECAR_FILE: &str = "workgraph.admission.sqlite3";

/// The sidecar lock path for a workgraph store under `state_dir`.
#[must_use]
pub fn workgraph_admission_sidecar_path(state_dir: &Path) -> PathBuf {
    state_dir.join(WORKGRAPH_ADMISSION_SIDECAR_FILE)
}

/// Late-bound slot through which a tool-plane dispatcher reaches the
/// runtime's [`WorkGraphAdmission`]. Created by
/// [`install_workgraph_tools`](crate::workgraph_wiring::install_workgraph_tools),
/// registered on the [`MobBootstrapSpec`](crate::MobBootstrapSpec), filled by
/// [`MobRuntime::bootstrap`](crate::MobRuntime::bootstrap). `None` (never
/// filled) means the embedder has no mob runtime; the dispatcher then
/// forwards without admission, exactly as before the guard existed.
pub type WorkGraphAdmissionSlot = Arc<std::sync::RwLock<Option<Arc<WorkGraphAdmission>>>>;

/// Why an admission was refused (or could not be decided).
#[derive(Debug)]
pub(crate) enum WorkGraphAdmissionError {
    /// The target already carries an occupying binding; `detail` names the
    /// occupying binding and the way out, and is safe to surface verbatim on
    /// both the RPC (K2 full-disclosure posture) and tool planes.
    Occupied { detail: String },
    /// The occupancy check itself failed against the service.
    Service(WorkGraphError),
    /// The cross-process sidecar lock could not be taken. Fail closed: an
    /// unserialized admission is exactly the race the sidecar exists to
    /// prevent.
    Lock(String),
}

/// Held for the whole check-then-mutate window of one admission decision.
/// Dropping it releases the in-process gate and (when configured) the
/// cross-process sidecar transaction.
pub(crate) struct WorkGraphAdmissionPermit {
    _in_process: tokio::sync::OwnedMutexGuard<()>,
    _cross_process: Option<SidecarLock>,
}

/// A `BEGIN IMMEDIATE` transaction held open on the sidecar database.
/// SQLite's RESERVED lock admits exactly one holder per file across
/// processes; dropping the connection rolls the (empty) transaction back and
/// releases the lock.
struct SidecarLock {
    _connection: rusqlite::Connection,
}

impl SidecarLock {
    /// Generous timeout: cross-process contention is rare (operator-paced
    /// goal/attention mutations), and failing closed on a busy sidecar
    /// refuses a legitimate admission.
    const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

    fn acquire(path: &Path) -> Result<Self, String> {
        let connection = rusqlite::Connection::open(path)
            .map_err(|error| format!("open admission sidecar {}: {error}", path.display()))?;
        connection
            .busy_timeout(Self::BUSY_TIMEOUT)
            .map_err(|error| format!("set admission sidecar busy timeout: {error}"))?;
        connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|error| {
                format!(
                    "could not lock the workgraph admission sidecar {} within the {}s busy \
                     timeout: {error}. The lock is held by another process sharing this state \
                     dir (in the documented deployment: a gateway and a library-mode runtime on \
                     one workgraph.sqlite3) — most likely a co-process is wedged mid-admission \
                     or under heavy binding-mutation load; retry, or check that co-process",
                    path.display(),
                    Self::BUSY_TIMEOUT.as_secs(),
                )
            })?;
        Ok(Self {
            _connection: connection,
        })
    }
}

/// Runtime-wide admission authority for attention-binding mutations: gate +
/// occupancy check. See the module docs for the invariants.
pub struct WorkGraphAdmission {
    mob_handle: meerkat_mob::MobHandle,
    /// Serializes every check-then-act window in this process. `Arc` so
    /// permits can hold an owned guard (the tool plane keeps one across the
    /// forwarded dispatch).
    gate: Arc<tokio::sync::Mutex<()>>,
    /// Cross-process lock database, set only for SQLite-backed stores —
    /// `workgraph.sqlite3` is documented as shareable by a gateway and a
    /// library-mode runtime on one state dir, and two processes means two
    /// in-process gates. Memory-backed runtimes are single-process by
    /// construction and keep the in-process gate only.
    sidecar: Option<PathBuf>,
}

impl WorkGraphAdmission {
    pub fn new(mob_handle: meerkat_mob::MobHandle, sidecar: Option<PathBuf>) -> Self {
        Self {
            mob_handle,
            gate: Arc::new(tokio::sync::Mutex::new(())),
            sidecar,
        }
    }

    /// The mob whose roster backs alias resolution (and whose definition id
    /// scopes identity-target lowering on the RPC surface).
    pub(crate) fn mob_handle(&self) -> &meerkat_mob::MobHandle {
        &self.mob_handle
    }

    /// WRITE-side target normalization (see the module docs): lower a
    /// session target that addresses one of this runtime's roster members to
    /// the member's owner form (`mob/<mob>/agent/<identity>`), so the stored
    /// binding row matches identity-form occupancy checks WITHOUT a roster —
    /// in the co-process sharing the store, and in this process while the
    /// member is mid-respawn. Non-member sessions keep their session form
    /// (their occupancy equivalence is raw-session-id equality; no aliasing
    /// exists for them), as does a member whose identity refuses to lower.
    pub(crate) async fn lower_member_session_target(
        &self,
        target: GoalAttentionTarget,
    ) -> GoalAttentionTarget {
        let GoalAttentionTarget::Session { session_id } = &target else {
            return target;
        };
        let roster = self.mob_handle.roster().await;
        let Some(entry) = roster.find_by_bridge_session_id(session_id) else {
            return target;
        };
        match meerkat_mob::lower_agent_identity_attention_target(
            &self.mob_handle.definition().id,
            &entry.agent_identity,
        ) {
            Ok(lowered) => lowered,
            Err(_) => target,
        }
    }

    /// Take the admission for one check-then-mutate window. The in-process
    /// gate is taken first so at most one task per process waits on the
    /// sidecar; the sidecar (when configured) then serializes against other
    /// processes sharing the store.
    pub(crate) async fn acquire(
        &self,
    ) -> Result<WorkGraphAdmissionPermit, WorkGraphAdmissionError> {
        let in_process = Arc::clone(&self.gate).lock_owned().await;
        let cross_process = match &self.sidecar {
            None => None,
            Some(path) => {
                let path = path.clone();
                let lock = tokio::task::spawn_blocking(move || SidecarLock::acquire(&path))
                    .await
                    .map_err(|error| {
                        WorkGraphAdmissionError::Lock(format!(
                            "admission sidecar lock task failed: {error}"
                        ))
                    })?
                    .map_err(WorkGraphAdmissionError::Lock)?;
                Some(lock)
            }
        };
        Ok(WorkGraphAdmissionPermit {
            _in_process: in_process,
            _cross_process: cross_process,
        })
    }

    /// Refuse a `goal/create`/`attention/reassign` whose target already
    /// carries an Active or Paused attention binding. Matching is primary
    /// owner-key equality first (roster-free — the write side normalizes
    /// member targets to owner form, see the module docs), with roster
    /// session↔identity aliasing as an extra layer for rows some other
    /// writer left in session form. `exclude` names the binding a reassign
    /// is superseding, which cannot conflict with its own move. Must be
    /// called with a permit held — the caller holds it across the mutation
    /// too.
    pub(crate) async fn check_target_free(
        &self,
        service: &WorkGraphService,
        namespace: Option<WorkNamespace>,
        target: &WorkAttentionTarget,
        exclude: Option<&WorkAttentionBindingId>,
        action: &str,
    ) -> Result<(), WorkGraphAdmissionError> {
        let aliases = attention_target_alias_keys(&self.mob_handle, target)
            .await
            .map_err(WorkGraphAdmissionError::Service)?;
        let bindings = list_occupying_attention(service, namespace)
            .await
            .map_err(WorkGraphAdmissionError::Service)?;
        let Some(existing) = bindings.iter().find(|binding| {
            exclude != Some(&binding.binding_id)
                && binding_occupies_target(&binding.status)
                && binding
                    .target
                    .owner_key()
                    .is_ok_and(|key| aliases.contains(&key.canonical()))
        }) else {
            return Ok(());
        };
        let target_key = target
            .owner_key()
            .map_err(WorkGraphAdmissionError::Service)?;
        Err(WorkGraphAdmissionError::Occupied {
            detail: match existing.status {
                WorkAttentionStatus::Paused { .. } => format!(
                    "target '{}' already has a paused attention binding {} that will reactivate \
                     when its pause expires; resume it or close its goal instead of {action}",
                    target_key.canonical(),
                    existing.binding_id,
                ),
                _ => format!(
                    "target '{}' already has an active attention binding {}; reassign it or \
                     close its goal before {action}",
                    target_key.canonical(),
                    existing.binding_id,
                ),
            },
        })
    }

    /// Resume-side twin of [`check_target_free`](Self::check_target_free):
    /// pause A, create B on the same member, resume A = two Active bindings.
    /// Siblings occupy exactly as on create/reassign — Active OR Paused (a
    /// timed pause auto-reactivates at expiry, so resuming "into" it just
    /// schedules the second Active); the resumed binding itself is excluded.
    /// An unknown `binding_id` falls through so the service reports its
    /// canonical not-found error.
    pub(crate) async fn check_resume_target_free(
        &self,
        service: &WorkGraphService,
        namespace: Option<WorkNamespace>,
        binding_id: &WorkAttentionBindingId,
    ) -> Result<(), WorkGraphAdmissionError> {
        let resumed = match service
            .attention_binding(AttentionBindingRequest {
                binding_id: binding_id.clone(),
                realm_id: None,
                namespace: namespace.clone(),
            })
            .await
        {
            Ok(result) => result.attention,
            Err(WorkGraphError::AttentionNotFound { .. }) => return Ok(()),
            Err(error) => return Err(WorkGraphAdmissionError::Service(error)),
        };
        let aliases = attention_target_alias_keys(&self.mob_handle, &resumed.target)
            .await
            .map_err(WorkGraphAdmissionError::Service)?;
        let siblings = list_occupying_attention(service, namespace)
            .await
            .map_err(WorkGraphAdmissionError::Service)?;
        let Some(other) = siblings.iter().find(|binding| {
            binding.binding_id != *binding_id
                && binding_occupies_target(&binding.status)
                && binding
                    .target
                    .owner_key()
                    .is_ok_and(|key| aliases.contains(&key.canonical()))
        }) else {
            return Ok(());
        };
        let target_key = resumed
            .target
            .owner_key()
            .map(|key| key.canonical())
            .unwrap_or_default();
        Err(WorkGraphAdmissionError::Occupied {
            detail: match other.status {
                WorkAttentionStatus::Paused { .. } => format!(
                    "resuming attention binding {binding_id} would give target '{target_key}' a \
                     second occupying binding: {} is paused and will reactivate when its pause \
                     expires; close its goal first",
                    other.binding_id,
                ),
                _ => format!(
                    "resuming attention binding {binding_id} would give target '{target_key}' a \
                     second active binding ({} is already active); reassign it or close its \
                     goal first",
                    other.binding_id,
                ),
            },
        })
    }
}

/// Whether `status` occupies its target: Active now, or Paused — a pause
/// auto-reactivates at expiry, and upstream's Active listing is
/// eligibility-at-now, so a paused binding is a scheduled second Active.
fn binding_occupies_target(status: &WorkAttentionStatus) -> bool {
    matches!(
        status,
        WorkAttentionStatus::Active | WorkAttentionStatus::Paused { .. }
    )
}

/// The bindings that currently occupy a target, queried once per occupying
/// status with the service scope pinned so upstream filters BEFORE returning
/// — an unfiltered `list_attention` would hand back every
/// permanently-accumulating Superseded/Stopped row on each admission, while
/// the global gate and sidecar are held. Upstream's `Active` filter is
/// eligibility-at-now (Active status, plus Paused past its deadline) and its
/// `Paused` filter is paused-and-not-yet-eligible, so the two scans are
/// disjoint and their union is exactly the Active-or-Paused set
/// [`binding_occupies_target`] admits; the callers keep that predicate as an
/// in-memory recheck so occupancy semantics do not silently follow upstream
/// filter drift. The store-level SELECT under these calls is still a full
/// scan (bounded by upstream — see the module docs).
async fn list_occupying_attention(
    service: &WorkGraphService,
    namespace: Option<WorkNamespace>,
) -> Result<Vec<WorkAttentionBinding>, WorkGraphError> {
    let namespace = namespace.unwrap_or_else(|| service.default_namespace().clone());
    let mut bindings = Vec::new();
    for status in [
        WorkAttentionStatus::Active,
        WorkAttentionStatus::Paused { until: None },
    ] {
        let result = service
            .list_attention(AttentionListRequest {
                realm_id: Some(service.default_realm_id().to_string()),
                namespace: Some(namespace.clone()),
                target: None,
                status: Some(status),
            })
            .await?;
        bindings.extend(result.attention);
    }
    Ok(bindings)
}

/// Mirror of upstream `mob_agent_owner_key_parts` (meerkat 0.7.23,
/// meerkat/src/surface.rs — private there): split a lowered
/// `mob/<mob>/agent/<identity>` owner id into its parts.
fn mob_agent_owner_key_parts(owner_id: &str) -> Option<(&str, &str)> {
    let rest = owner_id.strip_prefix("mob/")?;
    let (mob_id, agent_identity) = rest.split_once("/agent/")?;
    if mob_id.is_empty()
        || agent_identity.is_empty()
        || mob_id.contains('/')
        || agent_identity.contains('/')
    {
        return None;
    }
    Some((mob_id, agent_identity))
}

/// Every canonical owner-key spelling that addresses the same member as
/// `target`. Upstream `attention_target_matches_session` (meerkat 0.7.23,
/// meerkat/src/surface.rs) matches BOTH a member's bridge session id and its
/// lowered `mob/<mob>/agent/<identity>` owner key to the same member's
/// turns, so a session-form binding and an identity-form binding on one
/// member are still two bindings on one member. The primary key is always
/// present; the other spelling is added when the target resolves through
/// this runtime's roster (an unresolvable target simply has one spelling).
async fn attention_target_alias_keys(
    mob_handle: &meerkat_mob::MobHandle,
    target: &WorkAttentionTarget,
) -> Result<BTreeSet<String>, WorkGraphError> {
    let primary = target.owner_key()?;
    let mut keys = BTreeSet::from([primary.canonical()]);
    match primary.kind {
        // session → identity: the roster member owning this bridge session.
        WorkOwnerKind::Session => {
            if let Ok(session_id) = meerkat::SessionId::parse(&primary.id)
                && let Some(entry) = mob_handle
                    .roster()
                    .await
                    .find_by_bridge_session_id(&session_id)
                && let Ok(key) = meerkat_mob::lower_agent_identity_owner_key(
                    &mob_handle.definition().id,
                    &entry.agent_identity,
                )
            {
                keys.insert(key.canonical());
            }
        }
        // identity → session: only for THIS mob's lowered agent keys.
        WorkOwnerKind::Agent => {
            if let Some((mob_id, identity)) = mob_agent_owner_key_parts(&primary.id)
                && mob_id == mob_handle.definition().id.as_str()
                && let Some(session_id) = mob_handle
                    .resolve_bridge_session_id_observation(&meerkat_mob::ids::AgentIdentity::from(
                        identity,
                    ))
                    .await
                && let Ok(key) = WorkOwnerKey::session(session_id.to_string())
            {
                keys.insert(key.canonical());
            }
        }
        _ => {}
    }
    Ok(keys)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The sidecar mechanism itself: SQLite's `BEGIN IMMEDIATE` on one file
    /// admits exactly one holder — a second acquirer waits (busy handler)
    /// until the first releases. This is what serializes two PROCESSES that
    /// share one workgraph.sqlite3; the in-process gate cannot see them.
    #[tokio::test(flavor = "multi_thread")]
    async fn sidecar_lock_admits_one_holder_and_makes_the_second_wait() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = workgraph_admission_sidecar_path(dir.path());

        let first = SidecarLock::acquire(&path).expect("first lock");
        assert!(path.exists(), "acquire must create the sidecar database");

        let contended = path.clone();
        let second = tokio::task::spawn_blocking(move || SidecarLock::acquire(&contended));
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        assert!(
            !second.is_finished(),
            "second holder must wait while the first transaction is open"
        );

        drop(first);
        let second = second.await.expect("join");
        assert!(second.is_ok(), "released lock must admit the waiter");
    }

    /// The sidecar is a separate file from the store — holding a write
    /// transaction on workgraph.sqlite3 itself would deadlock the service's
    /// own writes mid-admission.
    #[test]
    fn sidecar_is_a_separate_file_from_the_store() {
        assert_eq!(
            WORKGRAPH_ADMISSION_SIDECAR_FILE,
            "workgraph.admission.sqlite3"
        );
        assert_ne!(
            WORKGRAPH_ADMISSION_SIDECAR_FILE,
            crate::workgraph_wiring::WORKGRAPH_STORE_FILE
        );
        let dir = Path::new("/state");
        assert_eq!(
            workgraph_admission_sidecar_path(dir),
            dir.join("workgraph.admission.sqlite3")
        );
    }
}
