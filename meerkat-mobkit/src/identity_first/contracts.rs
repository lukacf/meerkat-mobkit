//! Provider contracts for identity-first continuity.
//!
//! Five provider traits define the extension points:
//! - [`ContinuityStore`] — authoritative durable state (CONTRACT-01)
//! - [`LeaseProvider`] — live ownership / fencing tokens (CONTRACT-02)
//! - [`RosterProvider`] — roster/discovery (CONTRACT-03)
//! - [`AgentCustomizer`] — build-time agent customization (CONTRACT-04)
//! - [`TopologyProvider`] — managed dynamic topology edges (CONTRACT-05)

use std::collections::BTreeMap;

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

    /// Upsert a continuity record with fencing precondition.
    ///
    /// Rebinding an identity to a new session without changing
    /// `ContinuityGeneration` must not rewind `record.checkpoint_version`.
    async fn upsert_continuity_record(
        &self,
        record: &ContinuityRecord,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError>;

    /// Delete a continuity record and associated session snapshots.
    ///
    /// After deletion, `resolve_many` for this identity returns `Uninitialized`.
    /// Rejects stale fencing tokens.
    async fn delete_continuity_record(
        &self,
        identity: &AgentIdentity,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError>;
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
