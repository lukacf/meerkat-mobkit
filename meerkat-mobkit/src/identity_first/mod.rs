//! Identity-first continuity types and contracts for MobKit.

pub mod adapters;
pub mod bridge;
pub mod contracts;
pub mod gateway_bridges;
pub mod local_lease;
pub mod local_store;
pub mod orchestrator;
pub mod runtime;
mod types;

pub use adapters::{
    ContinuitySessionStoreAdapter, DiscoveryRosterAdapter, EdgeDiscoveryTopologyAdapter,
    SessionHookCustomizerAdapter, agent_discovery_to_durable,
};
pub use bridge::{BridgeError, MemberInspection, MobSessionBridge, SessionBridge};
pub use contracts::*;
pub use gateway_bridges::{
    CallbackBridge, GatewayAgentCustomizer, GatewayContinuityStore, GatewayLeaseProvider,
    GatewayRosterProvider, GatewayTopologyProvider,
};
pub use local_lease::LocalLeaseProvider;
pub use local_store::LocalContinuityStore;
pub use orchestrator::{
    ReconcileAction, RestoreFlowResult, RestoreOutcome, compute_reconcile_actions, restore_flow,
};
pub use runtime::{
    IdentityRuntime, IdentityRuntimeConfig, IdentityRuntimeError, wire_cross_mob_by_identity,
};
pub use types::*;
