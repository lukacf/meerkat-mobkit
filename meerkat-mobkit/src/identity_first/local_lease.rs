//! Bundled single-process LeaseProvider for `persistent_state(path)` usage.
//!
//! Implements CONTRACT-07. **Single-process only**: assumes one active runtime
//! per `persistent_state` directory. Multi-pod or failover deployments MUST
//! supply an external `LeaseProvider`.
//!
//! FencingTokens are monotonic within the local store. Leases are tracked
//! in-memory — there is no TTL enforcement (the single-process assumption
//! makes TTL-based expiry unnecessary).

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;

use super::contracts::LeaseProvider;
use super::types::{
    AgentIdentity, FencingToken, LeaseAcquireResult, LeaseError, LeaseGrant, LeaseRenewResult,
};

/// Default TTL returned by the local lease provider.
const LOCAL_LEASE_TTL: Duration = Duration::from_secs(300);

struct LeaseRecord {
    fencing_token: FencingToken,
    holder: String,
}

/// Single-process `LeaseProvider` for the bundled `persistent_state(path)` path.
///
/// **Single-process only.** Assumes one active runtime per persistent_state
/// directory. Does not provide distributed coordination. Multi-pod or failover
/// deployments must supply an external `LeaseProvider`.
pub struct LocalLeaseProvider {
    state: Mutex<LocalLeaseState>,
}

struct LocalLeaseState {
    next_token: u64,
    leases: BTreeMap<AgentIdentity, LeaseRecord>,
}

impl LocalLeaseProvider {
    /// Create a new local lease provider.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Mutex::new(LocalLeaseState {
                next_token: 1,
                leases: BTreeMap::new(),
            }),
        }
    }
}

impl Default for LocalLeaseProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LeaseProvider for LocalLeaseProvider {
    async fn acquire_leases(
        &self,
        identities: &[AgentIdentity],
        runtime_instance: &str,
    ) -> Result<BTreeMap<AgentIdentity, LeaseAcquireResult>, LeaseError> {
        let mut state = self
            .state
            .lock()
            .map_err(|e| LeaseError::Io(format!("lock: {e}")))?;
        let mut results = BTreeMap::new();
        for id in identities {
            if let Some(record) = state.leases.get(id)
                && record.holder != runtime_instance
            {
                results.insert(
                    id.clone(),
                    LeaseAcquireResult::AlreadyHeld {
                        identity: id.clone(),
                        holder: record.holder.clone(),
                    },
                );
                continue;
            }
            let token = FencingToken::new(state.next_token);
            state.next_token += 1;
            state.leases.insert(
                id.clone(),
                LeaseRecord {
                    fencing_token: token,
                    holder: runtime_instance.to_string(),
                },
            );
            results.insert(
                id.clone(),
                LeaseAcquireResult::Acquired(LeaseGrant {
                    identity: id.clone(),
                    fencing_token: token,
                    ttl: LOCAL_LEASE_TTL,
                }),
            );
        }
        Ok(results)
    }

    async fn renew_leases(
        &self,
        grants: &[LeaseGrant],
    ) -> Result<BTreeMap<AgentIdentity, LeaseRenewResult>, LeaseError> {
        let state = self
            .state
            .lock()
            .map_err(|e| LeaseError::Io(format!("lock: {e}")))?;
        let mut results = BTreeMap::new();
        for grant in grants {
            match state.leases.get(&grant.identity) {
                Some(record) if record.fencing_token == grant.fencing_token => {
                    results.insert(
                        grant.identity.clone(),
                        LeaseRenewResult::Renewed(LeaseGrant {
                            identity: grant.identity.clone(),
                            fencing_token: record.fencing_token,
                            ttl: LOCAL_LEASE_TTL,
                        }),
                    );
                }
                _ => {
                    results.insert(
                        grant.identity.clone(),
                        LeaseRenewResult::Lost {
                            identity: grant.identity.clone(),
                        },
                    );
                }
            }
        }
        Ok(results)
    }

    async fn release_leases(&self, grants: &[LeaseGrant]) -> Result<(), LeaseError> {
        let mut state = self
            .state
            .lock()
            .map_err(|e| LeaseError::Io(format!("lock: {e}")))?;
        for grant in grants {
            if let Some(record) = state.leases.get(&grant.identity)
                && record.fencing_token == grant.fencing_token
            {
                state.leases.remove(&grant.identity);
            }
        }
        Ok(())
    }
}
