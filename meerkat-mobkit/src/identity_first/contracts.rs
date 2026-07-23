//! Provider contracts for identity-first continuity.
//!
//! Five provider traits define the extension points:
//! - [`ContinuityStore`] — authoritative durable state (CONTRACT-01)
//! - [`LeaseProvider`] — live ownership / fencing tokens (CONTRACT-02)
//! - [`RosterProvider`] — roster/discovery (CONTRACT-03)
//! - [`AgentCustomizer`] — build-time agent customization (CONTRACT-04)
//! - [`TopologyProvider`] — managed dynamic topology edges (CONTRACT-05)

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;

use super::types::{
    AgentBuildContext, AgentBuildDraft, AgentIdentity, CheckpointVersion, ContinuityGeneration,
    ContinuityRecord, ContinuityResolveState, ContinuityStoreError, CustomizerError,
    DurableAgentSpec, FencingToken, LeaseAcquireResult, LeaseError, LeaseGrant, LeaseRenewResult,
    ManagedPeerEdge, RosterContext, RosterError, SessionSnapshot, TopologyContext, TopologyError,
};
use crate::mob_handle_runtime::SessionCreatedContext;

// ---------------------------------------------------------------------------
// CONTRACT-01: ContinuityStore
// ---------------------------------------------------------------------------

/// Candidate used by stores that can prove an exact, provenance-preserving
/// snapshot no-op without returning and reparsing the durable document.
///
/// A store may report a match only when the bytes, identity, generation, and
/// checkpoint revision identify its durable snapshot and the presented fence
/// is current write authority. The snapshot's stored fence is historical
/// provenance: it may precede the current lease epoch, but may never exceed
/// it. Implementations that cannot prove the complete predicate must use the
/// trait's default `false` response.
#[derive(Debug, Clone)]
pub struct SessionSnapshotMatchCandidate {
    pub identity: AgentIdentity,
    pub session_id: meerkat_core::types::SessionId,
    pub generation: ContinuityGeneration,
    pub checkpoint_version: CheckpointVersion,
    pub fencing_token: FencingToken,
    pub snapshot: Arc<SessionSnapshot>,
}

/// Authoritative durable state provider for identity-first continuity.
///
/// Implementations are responsible for persisting `ContinuityRecord`s and
/// `SessionSnapshot`s. The store treats `FencingToken` as an opaque monotonic
/// write-precondition — stale tokens are rejected via compare-and-set.
/// `CheckpointVersion` is the monotonic snapshot/version counter for
/// `(AgentIdentity, ContinuityGeneration)`: it advances across session
/// rotations and resets only when a destructive continuity reset advances the
/// generation. Session-local snapshot storage may still be keyed by
/// `SessionId`, but stale snapshot rejection must compare against the current
/// identity/generation head rather than treating a rebind as a fresh version
/// stream.
///
/// `resolve_many` MUST return an entry for every requested identity. Missing
/// entries are treated as a provider error, not implicit `Uninitialized`.
#[async_trait]
pub trait ContinuityStore: Send + Sync {
    /// Resolve continuity state for the given identities.
    ///
    /// Returns a `BTreeMap` with one entry per input identity.
    async fn resolve_many(
        &self,
        identities: &[AgentIdentity],
    ) -> Result<BTreeMap<AgentIdentity, ContinuityResolveState>, ContinuityStoreError>;

    /// Load a previously saved session snapshot.
    async fn load_session_snapshot(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<SessionSnapshot>, ContinuityStoreError>;

    /// Return `true` only when `candidate` is byte-for-byte identical to the
    /// current durable row and its identity, generation, and checkpoint
    /// version match. `candidate.fencing_token` must equal current write
    /// authority; the row's historical fence may be older but not newer.
    ///
    /// This additive capability lets adapters skip a full document load and
    /// parse for an already-durable save. The conservative default preserves
    /// compatibility and correctness for external stores.
    async fn session_snapshot_matches_current(
        &self,
        _candidate: SessionSnapshotMatchCandidate,
    ) -> Result<bool, ContinuityStoreError> {
        Ok(false)
    }

    /// Delete a saved session snapshot only if its serialized session
    /// projection still matches `expected_current_revision`.
    ///
    /// Implementations that cannot support session-scoped snapshot deletion
    /// must return `Ok(false)` rather than reporting a successful no-op.
    async fn delete_session_snapshot_if_current_revision(
        &self,
        _session_id: &meerkat_core::types::SessionId,
        _expected_current_revision: &str,
    ) -> Result<bool, ContinuityStoreError> {
        Ok(false)
    }

    /// Save a session snapshot with fencing and identity-generation version preconditions.
    async fn save_session_snapshot(
        &self,
        identity: &AgentIdentity,
        session_id: &meerkat_core::types::SessionId,
        generation: ContinuityGeneration,
        version: CheckpointVersion,
        fencing_token: FencingToken,
        snapshot: &SessionSnapshot,
    ) -> Result<(), ContinuityStoreError>;

    /// Owned equivalent of [`Self::save_session_snapshot`].
    ///
    /// Stores backed by blocking workers can override this method to move a
    /// large snapshot into the worker without cloning it. Existing providers
    /// inherit the compatibility implementation.
    async fn save_session_snapshot_owned(
        &self,
        identity: AgentIdentity,
        session_id: meerkat_core::types::SessionId,
        generation: ContinuityGeneration,
        version: CheckpointVersion,
        fencing_token: FencingToken,
        snapshot: SessionSnapshot,
    ) -> Result<(), ContinuityStoreError> {
        self.save_session_snapshot(
            &identity,
            &session_id,
            generation,
            version,
            fencing_token,
            &snapshot,
        )
        .await
    }

    /// Upsert a continuity record with fencing precondition.
    ///
    /// Rebinding an identity to a new session without changing
    /// `ContinuityGeneration` must not rewind `record.checkpoint_version`.
    async fn upsert_continuity_record(
        &self,
        record: &ContinuityRecord,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError>;

    /// Compensate a tentative continuity-generation advance only when the
    /// durable row still belongs to that exact reset attempt.
    ///
    /// Ordinary upserts are generation-monotonic and must reject an older
    /// generation even under a newer fence. Reset is the one workflow that
    /// can need a compensating rollback after publishing a provisional row so
    /// the persistent session service can construct the replacement. Restore
    /// therefore has to bypass monotonicity, and the only non-monotonic write
    /// verb the trait offers is deletion — the compatibility implementation
    /// deletes the attempt row (after validating it is still the attempt)
    /// and re-upserts `previous` as a fresh insert, which every conforming
    /// generation-monotonic store accepts.
    ///
    /// Documented compatibility caveats (why stores should override this
    /// with one atomic compare-and-swap transaction, as
    /// `LocalContinuityStore` does):
    ///
    /// - **Non-atomic**: a crash or a concurrent writer between the delete
    ///   and the re-upsert can leave the identity `Uninitialized`.
    /// - **Prior-generation snapshots are lost**: `delete_continuity_record`
    ///   removes the identity's session snapshots along with the record, so
    ///   the restored `previous` row comes back without its rollback-
    ///   authority snapshots; they must be re-established by the next
    ///   checkpoint. An atomic override deletes only the provisional
    ///   generation's snapshots and retains the previous generation's.
    async fn rollback_continuity_record(
        &self,
        expected_attempt: &ContinuityRecord,
        previous: Option<&ContinuityRecord>,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        let resolved = self
            .resolve_many(std::slice::from_ref(&expected_attempt.identity))
            .await?;
        let Some(ContinuityResolveState::Ready { record: current }) =
            resolved.get(&expected_attempt.identity)
        else {
            return Err(ContinuityStoreError::NotFound {
                identity: expected_attempt.identity.clone(),
            });
        };
        if current.agent_runtime_id != expected_attempt.agent_runtime_id
            || current.session_id != expected_attempt.session_id
            || current.generation != expected_attempt.generation
        {
            return Err(ContinuityStoreError::StaleContinuityGeneration {
                identity: expected_attempt.identity.clone(),
                presented: expected_attempt.generation,
                current: current.generation,
            });
        }
        // Delete-then-reinsert: the delete abandons the provisional row under
        // the fence CAS; the re-upsert of `previous` is a fresh insert, so a
        // conforming store's generation monotonicity (measured against the
        // current row) admits the older generation.
        self.delete_continuity_record(&expected_attempt.identity, fencing_token)
            .await?;
        match previous {
            Some(previous) => self.upsert_continuity_record(previous, fencing_token).await,
            None => Ok(()),
        }
    }

    /// Delete a continuity record and associated session snapshots.
    ///
    /// After deletion, `resolve_many` for this identity returns `Uninitialized`.
    /// Rejects stale fencing tokens.
    async fn delete_continuity_record(
        &self,
        identity: &AgentIdentity,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError>;

    /// Typed, discoverable capability: a session-delta channel compatible
    /// with meerkat's incremental persistence contract (O(delta) appends +
    /// CAS-guarded small head writes instead of a whole session document per
    /// save).
    ///
    /// Contract for stores that advertise this (return `Some`):
    ///
    /// - The returned store is a **session-granular authority over the same
    ///   durable state** the whole-snapshot verbs serve: a session persisted
    ///   through the incremental channel must load back through
    ///   [`Self::load_session_snapshot`]-backed reads (the committed-
    ///   authority resolver path) byte-consistently, and the CAS revision
    ///   tokens consumed by
    ///   [`Self::delete_session_snapshot_if_current_revision`] must remain
    ///   derivable from the current head + rows.
    /// - Every mutation must be enforced under the store's own continuity
    ///   write discipline — fencing-token compare-and-set and per
    ///   `(identity, generation)` version monotonicity apply per append and
    ///   per head write, not just per whole-blob save.
    ///
    /// The default is `None`: the store persists whole snapshots only, and
    /// `ContinuitySessionStoreAdapter::as_incremental` stays `None` (the H2
    /// loudly-reported whole-blob degradation). The bundled
    /// `LocalContinuityStore` deliberately returns `None` — see the M4b
    /// deferral note on that impl — and the JSON-RPC
    /// `GatewayContinuityStore` cannot advertise it (its wire protocol has
    /// only whole-snapshot verbs).
    fn as_incremental_sessions(
        &self,
    ) -> Option<Arc<dyn meerkat_core::session_store::IncrementalSessionStore>> {
        None
    }
}

// ---------------------------------------------------------------------------
// CONTRACT-02: LeaseProvider
// ---------------------------------------------------------------------------

/// Live ownership provider issuing monotonic fencing tokens.
///
/// The single source of truth for who may act on an identity now.
#[async_trait]
pub trait LeaseProvider: Send + Sync {
    /// Acquire leases for the given identities on behalf of `runtime_instance`.
    async fn acquire_leases(
        &self,
        identities: &[AgentIdentity],
        runtime_instance: &str,
    ) -> Result<BTreeMap<AgentIdentity, LeaseAcquireResult>, LeaseError>;

    /// Renew existing lease grants.
    ///
    /// Returning `Err` is a pre-commit failure: the provider must leave every
    /// supplied grant authoritative. Per-identity authority loss is reported as
    /// [`LeaseRenewResult::Lost`], and a committed rotation is returned as
    /// [`LeaseRenewResult::Renewed`] with the exact replacement token. This
    /// distinction lets runtimes safely quiesce and then either resume the old
    /// bridge authority or publish the returned replacement.
    async fn renew_leases(
        &self,
        grants: &[LeaseGrant],
    ) -> Result<BTreeMap<AgentIdentity, LeaseRenewResult>, LeaseError>;

    /// Release held leases.
    async fn release_leases(&self, grants: &[LeaseGrant]) -> Result<(), LeaseError>;
}

// ---------------------------------------------------------------------------
// CONTRACT-03: RosterProvider
// ---------------------------------------------------------------------------

/// Roster/discovery provider returning the desired set of durable agent specs.
#[async_trait]
pub trait RosterProvider: Send + Sync {
    /// Return the desired roster given the current context.
    async fn roster(&self, context: &RosterContext) -> Result<Vec<DurableAgentSpec>, RosterError>;
}

// ---------------------------------------------------------------------------
// CONTRACT-04: AgentCustomizer
// ---------------------------------------------------------------------------

/// Build-time agent customization provider.
///
/// `customize_build` runs before session creation. It receives read-only
/// context (identity, peers, topology) and the roster spec, and may mutate
/// the `AgentBuildDraft` (model, prompts, labels, tools, etc.).
///
/// Resume selection is NOT part of this contract — MobKit owns resume
/// injection after customize_build completes.
#[async_trait]
pub trait AgentCustomizer: Send + Sync {
    /// Customize the build draft for the given identity.
    async fn customize_build(
        &self,
        context: &AgentBuildContext,
        spec: &DurableAgentSpec,
        draft: &mut AgentBuildDraft,
    ) -> Result<(), CustomizerError>;

    /// Called after a session is successfully created. Best-effort.
    async fn after_create(
        &self,
        identity: &AgentIdentity,
        session_id: &meerkat_core::types::SessionId,
        context: &SessionCreatedContext,
    ) -> Result<(), CustomizerError> {
        let _ = (identity, session_id, context);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CONTRACT-05: TopologyProvider
// ---------------------------------------------------------------------------

/// Dynamic topology provider computing managed peer edges.
///
/// `target_identities` is the set of identities that WILL be active after
/// the current bootstrap/reconcile cycle — not just currently active ones.
#[async_trait]
pub trait TopologyProvider: Send + Sync {
    /// Compute the desired managed peer edges for the target activation set.
    async fn compute_edges(
        &self,
        target_identities: &[AgentIdentity],
        context: &TopologyContext,
    ) -> Result<Vec<ManagedPeerEdge>, TopologyError>;
}
