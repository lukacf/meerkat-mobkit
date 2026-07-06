//! Shared gateway construction seams (doctrine D2: converge construction,
//! merge binaries later).
//!
//! The K1/K2 field failures came from the two gateway binaries embedding the
//! same `UnifiedRuntime` with DIVERGENT wiring — `rpc_gateway` built the full
//! identity substrate, `mobkit_gateway` built none. Every piece both binaries
//! must construct identically belongs here so the drift class is structural
//! history. The full store construction converges with the 0.8 binary merge;
//! v1 shares the identity substrate, whose divergence caused the field
//! failures.

use std::path::Path;
use std::sync::Arc;

use crate::identity_first::{
    ContinuityStore, LocalContinuityStore, LocalLeaseProvider, contracts::LeaseProvider,
};

/// The durable-identity substrate a gateway builds from its state directory:
/// the continuity store and a lease provider whose fencing counter resumes
/// ABOVE the persisted high-water, so a restart with existing continuity
/// history never presents a stale token.
pub struct GatewayIdentitySubstrate {
    pub continuity_store: Arc<dyn ContinuityStore>,
    pub lease_provider: Arc<dyn LeaseProvider>,
}

/// Open the local identity substrate at `continuity_db`.
///
/// Fails loudly rather than degrading: a 0 fencing floor on an existing
/// store would re-arm the restart-abort class the floor seeding prevents.
pub fn open_identity_substrate(continuity_db: &Path) -> Result<GatewayIdentitySubstrate, String> {
    let store = LocalContinuityStore::open(continuity_db).map_err(|e| {
        format!(
            "failed to open continuity store {}: {e}",
            continuity_db.display()
        )
    })?;
    let fencing_floor = store
        .max_fencing_token()
        .map_err(|e| format!("failed to read continuity fencing high-water: {e}"))?;
    Ok(GatewayIdentitySubstrate {
        continuity_store: Arc::new(store),
        lease_provider: Arc::new(LocalLeaseProvider::with_floor(fencing_floor)),
    })
}
