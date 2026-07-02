//! Identity-first continuity types and contracts for MobKit.

pub mod adapters;
pub mod agent_memory;
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
pub use agent_memory::{
    AgentMemoryConfig, AgentMemoryCustomizer, AgentMemoryError, AgentMemoryForgetResult,
    AgentMemoryLlmWrites, AgentMemoryOperatorScope, AgentMemoryPerTurnInjection,
    AgentMemoryProvider, AgentMemoryRecallFailurePolicy, AgentMemoryRecallRequest,
    AgentMemoryRecord, AgentMemoryRuntimeInjector, AgentMemorySelection, AuthoredWriteReceipt,
    MEMORY_TOOL_NAME, MarkdownAgentMemoryStore, NewAgentMemory,
};
pub use bridge::{
    BridgeError, MemberInspection, MobSessionBridge, ResumeFallbackReason, ResumeSessionOutcome,
    SessionBridge,
};
pub use contracts::*;
pub use gateway_bridges::{
    CallbackBridge, GatewayAgentCustomizer, GatewayContinuityStore, GatewayLeaseProvider,
    GatewayRosterProvider, GatewayTopologyProvider,
};
pub use local_lease::LocalLeaseProvider;
pub use local_store::LocalContinuityStore;
pub use orchestrator::{
    ReconcileAction, RestoreFlowResult, RestoreOutcome, compute_reconcile_actions,
    lazy_register_flow, restore_flow,
};
pub use runtime::{
    IdentityFirstRuntimeContext, IdentityRuntime, IdentityRuntimeConfig, IdentityRuntimeError,
    wire_cross_mob_by_identity,
};
pub use types::*;
