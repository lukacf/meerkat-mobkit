//! Gateway protocol bridges: delegate identity-first provider operations to
//! Python/TypeScript SDKs via JSON-RPC callbacks over the stdio bridge.
//!
//! Each bridge implements the corresponding provider trait from `contracts.rs` and
//! delegates to the SDK by sending a JSON-RPC request through `StdioCallbackBridge`
//! and deserializing the response.
//!
//! Wire format conventions (REQ-34 through REQ-39):
//! - AgentIdentity: string
//! - FencingToken / CheckpointVersion / ContinuityGeneration: integer
//! - LeaseGrant.ttl: integer milliseconds
//! - SessionSnapshot.data: base64 string
//! - All operations use request/response JSON-RPC (no notification-only failure paths)
//! - Errors propagate as JSON-RPC errors → typed Rust errors

use std::collections::BTreeMap;
use std::sync::Arc;

use async_trait::async_trait;
use meerkat_core::agent::AgentToolDispatcher;
use meerkat_core::types::ToolCallView;
use meerkat_core::{ContentBlock, ToolDef, ToolResult, error::ToolError, ops::ToolDispatchOutcome};
use serde_json::{Value, json};

use super::contracts::{
    AgentCustomizer, ContinuityStore, LeaseProvider, RosterProvider, TopologyProvider,
};
use super::types::{
    AgentBuildContext, AgentBuildDraft, AgentIdentity, CheckpointVersion, ContinuityGeneration,
    ContinuityResolveState, ContinuityStoreError, CustomizerError, DurableAgentSpec, FencingToken,
    LeaseAcquireResult, LeaseError, LeaseGrant, LeaseRenewResult, LocalExternalToolOverlay,
    ManagedPeerEdge, RosterContext, RosterError, SessionSnapshot, TopologyContext, TopologyError,
};
use crate::mob_handle_runtime::SessionCreatedContext;

// ---------------------------------------------------------------------------
// CallbackBridge trait — abstracts the stdio callback mechanism for testability
// ---------------------------------------------------------------------------

/// Trait abstracting the JSON-RPC callback mechanism.
///
/// In production this is `StdioCallbackBridge` from `rpc_gateway.rs`.
/// In tests this can be replaced with a mock.
#[async_trait]
pub trait CallbackBridge: Send + Sync {
    /// Send a JSON-RPC request and wait for the response.
    async fn call(&self, method: &str, params: Value) -> Result<Value, String>;
}

// ---------------------------------------------------------------------------
// REQ-35: Gateway ContinuityStore bridge
// ---------------------------------------------------------------------------

/// Gateway-side `ContinuityStore` that delegates to Python/TypeScript via JSON-RPC.
pub struct GatewayContinuityStore {
    bridge: Box<dyn CallbackBridge>,
}

impl GatewayContinuityStore {
    pub fn new(bridge: impl CallbackBridge + 'static) -> Self {
        Self {
            bridge: Box::new(bridge),
        }
    }
}

#[async_trait]
impl ContinuityStore for GatewayContinuityStore {
    async fn resolve_many(
        &self,
        identities: &[AgentIdentity],
    ) -> Result<BTreeMap<AgentIdentity, ContinuityResolveState>, ContinuityStoreError> {
        let id_strings: Vec<&str> = identities.iter().map(AgentIdentity::as_str).collect();
        let params = json!({ "identities": id_strings });
        let result = self
            .bridge
            .call("callback/continuity_store/resolve_many", params)
            .await
            .map_err(ContinuityStoreError::Io)?;

        let map: BTreeMap<AgentIdentity, ContinuityResolveState> =
            serde_json::from_value(result)
                .map_err(|e| ContinuityStoreError::Io(format!("deserialize resolve_many: {e}")))?;
        Ok(map)
    }

    async fn load_session_snapshot(
        &self,
        session_id: &meerkat_core::types::SessionId,
    ) -> Result<Option<SessionSnapshot>, ContinuityStoreError> {
        let params = json!({ "session_id": session_id.to_string() });
        let result = self
            .bridge
            .call("callback/continuity_store/load_session_snapshot", params)
            .await
            .map_err(ContinuityStoreError::Io)?;

        if result.is_null() {
            return Ok(None);
        }

        let snapshot: SessionSnapshot = serde_json::from_value(result)
            .map_err(|e| ContinuityStoreError::Io(format!("deserialize snapshot: {e}")))?;
        Ok(Some(snapshot))
    }

    async fn delete_session_snapshot_if_current_revision(
        &self,
        session_id: &meerkat_core::types::SessionId,
        expected_current_revision: &str,
    ) -> Result<bool, ContinuityStoreError> {
        let params = json!({
            "session_id": session_id.to_string(),
            "expected_current_revision": expected_current_revision,
        });
        let result = self
            .bridge
            .call(
                "callback/continuity_store/delete_session_snapshot_if_current_revision",
                params,
            )
            .await;

        match result {
            Ok(value) => serde_json::from_value(value).map_err(|e| {
                ContinuityStoreError::Io(format!(
                    "deserialize delete_session_snapshot_if_current_revision: {e}"
                ))
            }),
            Err(e)
                if e.contains("unknown continuity_store operation")
                    || e.contains("method not found")
                    || e.contains("not implemented") =>
            {
                Ok(false)
            }
            Err(e) => Err(parse_continuity_store_error(&e)),
        }
    }

    async fn save_session_snapshot(
        &self,
        identity: &AgentIdentity,
        session_id: &meerkat_core::types::SessionId,
        generation: ContinuityGeneration,
        version: CheckpointVersion,
        fencing_token: FencingToken,
        snapshot: &SessionSnapshot,
    ) -> Result<(), ContinuityStoreError> {
        let params = json!({
            "identity": identity.as_str(),
            "session_id": session_id.to_string(),
            "generation": generation.get(),
            "version": version.get(),
            "fencing_token": fencing_token.get(),
            "snapshot": serde_json::to_value(snapshot)
                .map_err(|e| ContinuityStoreError::Io(format!("serialize snapshot: {e}")))?,
        });
        let result = self
            .bridge
            .call("callback/continuity_store/save_session_snapshot", params)
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(parse_continuity_store_error(&e)),
        }
    }

    async fn upsert_continuity_record(
        &self,
        record: &super::types::ContinuityRecord,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        let params = json!({
            "record": serde_json::to_value(record)
                .map_err(|e| ContinuityStoreError::Io(format!("serialize record: {e}")))?,
            "fencing_token": fencing_token.get(),
        });
        let result = self
            .bridge
            .call("callback/continuity_store/upsert_continuity_record", params)
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(parse_continuity_store_error(&e)),
        }
    }

    async fn delete_continuity_record(
        &self,
        identity: &AgentIdentity,
        fencing_token: FencingToken,
    ) -> Result<(), ContinuityStoreError> {
        let params = json!({
            "identity": identity.as_str(),
            "fencing_token": fencing_token.get(),
        });
        let result = self
            .bridge
            .call("callback/continuity_store/delete_continuity_record", params)
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => Err(parse_continuity_store_error(&e)),
        }
    }
}

/// Parse a callback error string into a typed `ContinuityStoreError`.
///
/// The SDK-side provider is expected to include structured error info in the
/// error message. We do best-effort parsing here.
fn parse_continuity_store_error(err: &str) -> ContinuityStoreError {
    if err.contains("stale_fencing_token") || err.contains("StaleFencingToken") {
        // The SDK should include structured error details; fall back to Io
        // if we can't parse them.
        ContinuityStoreError::Io(format!("stale fencing token: {err}"))
    } else if err.contains("stale_checkpoint_version") || err.contains("StaleCheckpointVersion") {
        ContinuityStoreError::Io(format!("stale checkpoint version: {err}"))
    } else if err.contains("not_found") || err.contains("NotFound") {
        ContinuityStoreError::Io(format!("not found: {err}"))
    } else if err.contains("corruption") || err.contains("Corruption") {
        ContinuityStoreError::Corruption(err.to_string())
    } else {
        ContinuityStoreError::Io(err.to_string())
    }
}

// ---------------------------------------------------------------------------
// REQ-36: Gateway LeaseProvider bridge
// ---------------------------------------------------------------------------

/// Gateway-side `LeaseProvider` that delegates to Python/TypeScript via JSON-RPC.
pub struct GatewayLeaseProvider {
    bridge: Box<dyn CallbackBridge>,
}

impl GatewayLeaseProvider {
    pub fn new(bridge: impl CallbackBridge + 'static) -> Self {
        Self {
            bridge: Box::new(bridge),
        }
    }
}

#[async_trait]
impl LeaseProvider for GatewayLeaseProvider {
    async fn acquire_leases(
        &self,
        identities: &[AgentIdentity],
        runtime_instance: &str,
    ) -> Result<BTreeMap<AgentIdentity, LeaseAcquireResult>, LeaseError> {
        let id_strings: Vec<&str> = identities.iter().map(AgentIdentity::as_str).collect();
        let params = json!({
            "identities": id_strings,
            "runtime_instance": runtime_instance,
        });
        let result = self
            .bridge
            .call("callback/lease_provider/acquire_leases", params)
            .await
            .map_err(LeaseError::Io)?;

        serde_json::from_value(result)
            .map_err(|e| LeaseError::Io(format!("deserialize acquire_leases: {e}")))
    }

    async fn renew_leases(
        &self,
        grants: &[LeaseGrant],
    ) -> Result<BTreeMap<AgentIdentity, LeaseRenewResult>, LeaseError> {
        let grants_json = serde_json::to_value(grants)
            .map_err(|e| LeaseError::Io(format!("serialize grants: {e}")))?;
        let params = json!({ "grants": grants_json });
        let result = self
            .bridge
            .call("callback/lease_provider/renew_leases", params)
            .await
            .map_err(LeaseError::Io)?;

        serde_json::from_value(result)
            .map_err(|e| LeaseError::Io(format!("deserialize renew_leases: {e}")))
    }

    async fn release_leases(&self, grants: &[LeaseGrant]) -> Result<(), LeaseError> {
        let grants_json = serde_json::to_value(grants)
            .map_err(|e| LeaseError::Io(format!("serialize grants: {e}")))?;
        let params = json!({ "grants": grants_json });
        self.bridge
            .call("callback/lease_provider/release_leases", params)
            .await
            .map_err(LeaseError::Io)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// REQ-37: Gateway RosterProvider bridge
// ---------------------------------------------------------------------------

/// Gateway-side `RosterProvider` that delegates to Python/TypeScript via JSON-RPC.
pub struct GatewayRosterProvider {
    bridge: Box<dyn CallbackBridge>,
}

impl GatewayRosterProvider {
    pub fn new(bridge: impl CallbackBridge + 'static) -> Self {
        Self {
            bridge: Box::new(bridge),
        }
    }
}

#[async_trait]
impl RosterProvider for GatewayRosterProvider {
    async fn roster(&self, context: &RosterContext) -> Result<Vec<DurableAgentSpec>, RosterError> {
        let params = serde_json::to_value(context)
            .map_err(|e| RosterError::Io(format!("serialize roster context: {e}")))?;
        let result = self
            .bridge
            .call("callback/roster_provider/roster", params)
            .await
            .map_err(RosterError::Io)?;

        serde_json::from_value(result)
            .map_err(|e| RosterError::Io(format!("deserialize roster: {e}")))
    }
}

// ---------------------------------------------------------------------------
// REQ-38: Gateway AgentCustomizer bridge
// ---------------------------------------------------------------------------

/// Gateway-side `AgentCustomizer` that delegates to Python/TypeScript via JSON-RPC.
pub struct GatewayAgentCustomizer {
    bridge: Arc<dyn CallbackBridge>,
}

impl GatewayAgentCustomizer {
    pub fn new(bridge: impl CallbackBridge + 'static) -> Self {
        Self {
            bridge: Arc::new(bridge),
        }
    }
}

struct GatewayCallbackToolDispatcher {
    bridge: Arc<dyn CallbackBridge>,
    scope_id: String,
    tool_defs: Arc<[Arc<ToolDef>]>,
}

impl GatewayCallbackToolDispatcher {
    fn new(
        bridge: Arc<dyn CallbackBridge>,
        scope_id: String,
        external_tools: Vec<super::types::ExternalToolDef>,
    ) -> Self {
        let tool_defs = external_tools
            .into_iter()
            .map(|tool| {
                Arc::new(ToolDef {
                    name: tool.name.into(),
                    description: tool.description,
                    input_schema: tool.input_schema,
                    provenance: None,
                })
            })
            .collect::<Vec<_>>()
            .into();
        Self {
            bridge,
            scope_id,
            tool_defs,
        }
    }
}

#[async_trait]
impl AgentToolDispatcher for GatewayCallbackToolDispatcher {
    fn tools(&self) -> Arc<[Arc<ToolDef>]> {
        self.tool_defs.clone()
    }

    async fn dispatch(&self, call: ToolCallView<'_>) -> Result<ToolDispatchOutcome, ToolError> {
        let args: Value =
            serde_json::from_str(call.args.get()).map_err(|err| ToolError::InvalidArguments {
                name: call.name.to_string(),
                reason: err.to_string(),
            })?;
        let params = json!({
            "scope_id": self.scope_id,
            "tool": call.name,
            "arguments": args,
        });
        match self.bridge.call("callback/call_tool", params).await {
            Ok(result) => {
                let text = result
                    .get("content")
                    .map(|value| {
                        value
                            .as_str()
                            .map(ToString::to_string)
                            .unwrap_or_else(|| serde_json::to_string(value).unwrap_or_default())
                    })
                    .unwrap_or_else(|| serde_json::to_string(&result).unwrap_or_default());
                Ok(ToolResult {
                    tool_use_id: call.id.to_string(),
                    content: vec![ContentBlock::Text { text }],
                    is_error: false,
                }
                .into())
            }
            Err(err) => Ok(ToolResult {
                tool_use_id: call.id.to_string(),
                content: vec![ContentBlock::Text {
                    text: format!("Tool execution failed: {err}"),
                }],
                is_error: true,
            }
            .into()),
        }
    }
}

#[async_trait]
impl AgentCustomizer for GatewayAgentCustomizer {
    async fn customize_build(
        &self,
        context: &AgentBuildContext,
        spec: &DurableAgentSpec,
        draft: &mut AgentBuildDraft,
    ) -> Result<(), CustomizerError> {
        let params = json!({
            "context": serde_json::to_value(context)
                .map_err(|e| CustomizerError::Io(format!("serialize context: {e}")))?,
            "spec": serde_json::to_value(spec)
                .map_err(|e| CustomizerError::Io(format!("serialize spec: {e}")))?,
            "draft": serde_json::to_value(&*draft)
                .map_err(|e| CustomizerError::Io(format!("serialize draft: {e}")))?,
        });
        let result = self
            .bridge
            .call("callback/agent_customizer/customize_build", params)
            .await
            .map_err(CustomizerError::Io)?;

        // The SDK returns the mutated draft. Deserialize and apply.
        let mut returned_draft: AgentBuildDraft = serde_json::from_value(result)
            .map_err(|e| CustomizerError::Io(format!("deserialize draft: {e}")))?;
        if !returned_draft.external_tools.is_empty() {
            returned_draft.local_external_tools =
                LocalExternalToolOverlay::new(Arc::new(GatewayCallbackToolDispatcher::new(
                    self.bridge.clone(),
                    context.identity.as_str().to_string(),
                    returned_draft.external_tools.clone(),
                )));
        }
        *draft = returned_draft;
        Ok(())
    }

    async fn after_create(
        &self,
        identity: &AgentIdentity,
        session_id: &meerkat_core::types::SessionId,
        context: &SessionCreatedContext,
    ) -> Result<(), CustomizerError> {
        let params = json!({
            "identity": identity.as_str(),
            "session_id": session_id.to_string(),
            "model": context.model,
            "labels": context.labels,
            "system_prompt": context.system_prompt,
        });
        self.bridge
            .call("callback/agent_customizer/after_create", params)
            .await
            .map_err(CustomizerError::Io)?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// REQ-39: Gateway TopologyProvider bridge
// ---------------------------------------------------------------------------

/// Gateway-side `TopologyProvider` that delegates to Python/TypeScript via JSON-RPC.
pub struct GatewayTopologyProvider {
    bridge: Box<dyn CallbackBridge>,
}

impl GatewayTopologyProvider {
    pub fn new(bridge: impl CallbackBridge + 'static) -> Self {
        Self {
            bridge: Box::new(bridge),
        }
    }
}

#[async_trait]
impl TopologyProvider for GatewayTopologyProvider {
    async fn compute_edges(
        &self,
        target_identities: &[AgentIdentity],
        context: &TopologyContext,
    ) -> Result<Vec<ManagedPeerEdge>, TopologyError> {
        let id_strings: Vec<&str> = target_identities
            .iter()
            .map(AgentIdentity::as_str)
            .collect();
        let params = json!({
            "target_identities": id_strings,
            "context": serde_json::to_value(context)
                .map_err(|e| TopologyError::ProviderUnavailable(
                    format!("serialize topology context: {e}")
                ))?,
        });
        let result = self
            .bridge
            .call("callback/topology_provider/compute_edges", params)
            .await
            .map_err(TopologyError::ProviderUnavailable)?;

        serde_json::from_value(result)
            .map_err(|e| TopologyError::InvalidEdge(format!("deserialize compute_edges: {e}")))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    use super::super::types::{
        AgentAddressability, AgentRuntimeId, CheckpointVersion, ContinuityGeneration,
        ContinuityRecord, ContinuityResolveState, ExternalToolDef, FencingToken, LeaseGrant,
        ManagedPeerEdge, SessionSnapshot,
    };

    // -----------------------------------------------------------------------
    // Mock callback bridge
    // -----------------------------------------------------------------------

    /// Records calls and returns pre-configured responses.
    struct MockBridge {
        /// (method, params) for each call
        calls: Arc<Mutex<Vec<(String, Value)>>>,
        /// Response to return, keyed by method. Falls back to `Value::Null`.
        responses: Arc<Mutex<std::collections::HashMap<String, Result<Value, String>>>>,
    }

    impl MockBridge {
        fn new() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(std::collections::HashMap::new())),
            }
        }

        async fn set_response(&self, method: &str, response: Result<Value, String>) {
            self.responses
                .lock()
                .await
                .insert(method.to_string(), response);
        }

        async fn last_call(&self) -> (String, Value) {
            self.calls
                .lock()
                .await
                .last()
                .cloned()
                .unwrap_or_else(|| (String::new(), Value::Null))
        }

        #[allow(dead_code)]
        async fn call_count(&self) -> usize {
            self.calls.lock().await.len()
        }
    }

    #[async_trait]
    impl CallbackBridge for Arc<MockBridge> {
        async fn call(&self, method: &str, params: Value) -> Result<Value, String> {
            self.calls.lock().await.push((method.to_string(), params));
            let responses = self.responses.lock().await;
            match responses.get(method) {
                Some(Ok(v)) => Ok(v.clone()),
                Some(Err(e)) => Err(e.clone()),
                None => Ok(Value::Null),
            }
        }
    }

    // -----------------------------------------------------------------------
    // REQ-34: Structured RPC — error propagation
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_identity_first_gateway_structured_rpc_error_propagation() {
        // REQ-34: Errors from provider operations must propagate as typed errors,
        // not be swallowed as notification-only.
        let mock = Arc::new(MockBridge::new());
        mock.set_response(
            "callback/continuity_store/resolve_many",
            Err("callback error: store unavailable".to_string()),
        )
        .await;

        let store = GatewayContinuityStore::new(mock.clone());
        let id = AgentIdentity::parse("test:agent").unwrap();
        let result = store.resolve_many(&[id]).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            ContinuityStoreError::Io(msg) => {
                assert!(msg.contains("store unavailable"), "error: {msg}");
            }
            other => panic!("expected Io error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_identity_first_gateway_lease_error_propagation() {
        let mock = Arc::new(MockBridge::new());
        mock.set_response(
            "callback/lease_provider/acquire_leases",
            Err("provider unavailable".to_string()),
        )
        .await;

        let provider = GatewayLeaseProvider::new(mock.clone());
        let id = AgentIdentity::parse("test:agent").unwrap();
        let result = provider.acquire_leases(&[id], "runtime-1").await;
        assert!(result.is_err());
        match result.unwrap_err() {
            LeaseError::Io(msg) => {
                assert!(msg.contains("provider unavailable"), "error: {msg}");
            }
            other => panic!("expected Io error, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_identity_first_gateway_roster_error_propagation() {
        let mock = Arc::new(MockBridge::new());
        mock.set_response(
            "callback/roster_provider/roster",
            Err("roster failed".to_string()),
        )
        .await;

        let provider = GatewayRosterProvider::new(mock.clone());
        let ctx = RosterContext {
            mob_definition: None,
            previous_identities: vec![],
        };
        let result = provider.roster(&ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_identity_first_gateway_customizer_error_propagation() {
        let mock = Arc::new(MockBridge::new());
        mock.set_response(
            "callback/agent_customizer/customize_build",
            Err("build failed".to_string()),
        )
        .await;

        let customizer = GatewayAgentCustomizer::new(mock.clone());
        let id = AgentIdentity::parse("test:agent").unwrap();
        let context = AgentBuildContext {
            identity: id.clone(),
            active_peers: vec![],
            managed_edges: vec![],
            runtime_services: Default::default(),
        };
        let spec = DurableAgentSpec {
            identity: id,
            profile: meerkat_mob::ProfileName::from("default"),
            addressability: AgentAddressability::Addressable,
            display_name: None,
            labels: BTreeMap::new(),
            context: None,
            additional_instructions: vec![],
            initial_message: None,
            runtime_mode_override: None,
            backend: None,
            binding: None,
        };
        let mut draft = AgentBuildDraft {
            model: None,
            system_prompt: None,
            additional_instructions: vec![],
            labels: BTreeMap::new(),
            app_context: None,
            external_tools: vec![],
            local_external_tools: Default::default(),
        };
        let result = customizer
            .customize_build(&context, &spec, &mut draft)
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_identity_first_gateway_topology_error_propagation() {
        let mock = Arc::new(MockBridge::new());
        mock.set_response(
            "callback/topology_provider/compute_edges",
            Err("topology unavailable".to_string()),
        )
        .await;

        let provider = GatewayTopologyProvider::new(mock.clone());
        let id = AgentIdentity::parse("test:agent").unwrap();
        let ctx = TopologyContext { roster: vec![] };
        let result = provider.compute_edges(&[id], &ctx).await;
        assert!(result.is_err());
    }

    // -----------------------------------------------------------------------
    // REQ-35: ContinuityStore bridge
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_identity_first_gateway_continuity_store_resolve_many() {
        let mock = Arc::new(MockBridge::new());

        // Set up response: one Ready, one Uninitialized
        let record = ContinuityRecord {
            identity: AgentIdentity::parse("agent:alpha").unwrap(),
            agent_runtime_id: AgentRuntimeId::parse("rt-001").unwrap(),
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(5),
        };
        let mut response_map = BTreeMap::new();
        response_map.insert(
            AgentIdentity::parse("agent:alpha").unwrap(),
            ContinuityResolveState::Ready {
                record: record.clone(),
            },
        );
        response_map.insert(
            AgentIdentity::parse("agent:beta").unwrap(),
            ContinuityResolveState::Uninitialized,
        );
        mock.set_response(
            "callback/continuity_store/resolve_many",
            Ok(serde_json::to_value(&response_map).unwrap()),
        )
        .await;

        let store = GatewayContinuityStore::new(mock.clone());
        let ids = vec![
            AgentIdentity::parse("agent:alpha").unwrap(),
            AgentIdentity::parse("agent:beta").unwrap(),
        ];
        let result = store.resolve_many(&ids).await.unwrap();
        assert_eq!(result.len(), 2);

        // Verify identity strings were sent correctly
        let (method, params) = mock.last_call().await;
        assert_eq!(method, "callback/continuity_store/resolve_many");
        let sent_ids = params["identities"].as_array().unwrap();
        assert_eq!(sent_ids[0].as_str().unwrap(), "agent:alpha");
        assert_eq!(sent_ids[1].as_str().unwrap(), "agent:beta");

        // Verify deserialized correctly
        match &result[&AgentIdentity::parse("agent:alpha").unwrap()] {
            ContinuityResolveState::Ready { record: r } => {
                assert_eq!(r.identity, record.identity);
                assert_eq!(r.checkpoint_version, CheckpointVersion::new(5));
            }
            other => panic!("expected Ready, got: {other:?}"),
        }
        assert!(matches!(
            result[&AgentIdentity::parse("agent:beta").unwrap()],
            ContinuityResolveState::Uninitialized
        ));
    }

    #[tokio::test]
    async fn test_identity_first_gateway_continuity_store_load_snapshot() {
        let mock = Arc::new(MockBridge::new());
        let snapshot = SessionSnapshot {
            data: b"test session data".to_vec(),
        };
        mock.set_response(
            "callback/continuity_store/load_session_snapshot",
            Ok(serde_json::to_value(&snapshot).unwrap()),
        )
        .await;

        let store = GatewayContinuityStore::new(mock.clone());
        let sid = meerkat_core::types::SessionId::new();
        let result = store.load_session_snapshot(&sid).await.unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().data, b"test session data");

        // Verify session_id was sent as string
        let (_, params) = mock.last_call().await;
        assert_eq!(params["session_id"].as_str().unwrap(), sid.to_string());
    }

    #[tokio::test]
    async fn test_identity_first_gateway_continuity_store_load_snapshot_null() {
        let mock = Arc::new(MockBridge::new());
        // null response means no snapshot found
        mock.set_response(
            "callback/continuity_store/load_session_snapshot",
            Ok(Value::Null),
        )
        .await;

        let store = GatewayContinuityStore::new(mock.clone());
        let sid = meerkat_core::types::SessionId::new();
        let result = store.load_session_snapshot(&sid).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_identity_first_gateway_continuity_store_save_snapshot() {
        let mock = Arc::new(MockBridge::new());
        mock.set_response(
            "callback/continuity_store/save_session_snapshot",
            Ok(Value::Null),
        )
        .await;

        let store = GatewayContinuityStore::new(mock.clone());
        let id = AgentIdentity::parse("agent:alpha").unwrap();
        let sid = meerkat_core::types::SessionId::new();
        let snapshot = SessionSnapshot {
            data: b"checkpoint data".to_vec(),
        };

        store
            .save_session_snapshot(
                &id,
                &sid,
                ContinuityGeneration::new(2),
                CheckpointVersion::new(7),
                FencingToken::new(42),
                &snapshot,
            )
            .await
            .unwrap();

        // Verify wire format: newtypes serialize as expected
        let (method, params) = mock.last_call().await;
        assert_eq!(method, "callback/continuity_store/save_session_snapshot");
        assert_eq!(params["identity"].as_str().unwrap(), "agent:alpha");
        assert_eq!(params["session_id"].as_str().unwrap(), sid.to_string());
        assert_eq!(params["generation"].as_u64().unwrap(), 2);
        assert_eq!(params["version"].as_u64().unwrap(), 7);
        assert_eq!(params["fencing_token"].as_u64().unwrap(), 42);
        // snapshot should be {"data": "<base64>"} per spec TYPE-22
        assert!(params["snapshot"].is_object());
        assert!(
            params["snapshot"]
                .get("data")
                .and_then(|d| d.as_str())
                .is_some()
        );
    }

    #[tokio::test]
    async fn test_identity_first_gateway_continuity_store_save_snapshot_error() {
        let mock = Arc::new(MockBridge::new());
        mock.set_response(
            "callback/continuity_store/save_session_snapshot",
            Err("callback error: stale_fencing_token".to_string()),
        )
        .await;

        let store = GatewayContinuityStore::new(mock.clone());
        let id = AgentIdentity::parse("agent:alpha").unwrap();
        let sid = meerkat_core::types::SessionId::new();
        let snapshot = SessionSnapshot {
            data: vec![1, 2, 3],
        };
        let result = store
            .save_session_snapshot(
                &id,
                &sid,
                ContinuityGeneration::new(0),
                CheckpointVersion::new(1),
                FencingToken::new(1),
                &snapshot,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_identity_first_gateway_continuity_store_upsert_record() {
        let mock = Arc::new(MockBridge::new());
        mock.set_response(
            "callback/continuity_store/upsert_continuity_record",
            Ok(Value::Null),
        )
        .await;

        let store = GatewayContinuityStore::new(mock.clone());
        let record = ContinuityRecord {
            identity: AgentIdentity::parse("agent:alpha").unwrap(),
            agent_runtime_id: AgentRuntimeId::parse("rt-001").unwrap(),
            session_id: meerkat_core::types::SessionId::new(),
            generation: ContinuityGeneration::new(0),
            checkpoint_version: CheckpointVersion::new(0),
        };
        store
            .upsert_continuity_record(&record, FencingToken::new(10))
            .await
            .unwrap();

        let (_, params) = mock.last_call().await;
        assert_eq!(params["fencing_token"].as_u64().unwrap(), 10);
        // record should be a nested object
        assert!(params["record"].is_object());
        assert_eq!(
            params["record"]["identity"].as_str().unwrap(),
            "agent:alpha"
        );
    }

    #[tokio::test]
    async fn test_identity_first_gateway_continuity_store_delete_record() {
        let mock = Arc::new(MockBridge::new());
        mock.set_response(
            "callback/continuity_store/delete_continuity_record",
            Ok(Value::Null),
        )
        .await;

        let store = GatewayContinuityStore::new(mock.clone());
        let id = AgentIdentity::parse("agent:alpha").unwrap();
        store
            .delete_continuity_record(&id, FencingToken::new(99))
            .await
            .unwrap();

        let (method, params) = mock.last_call().await;
        assert_eq!(method, "callback/continuity_store/delete_continuity_record");
        assert_eq!(params["identity"].as_str().unwrap(), "agent:alpha");
        assert_eq!(params["fencing_token"].as_u64().unwrap(), 99);
    }

    // -----------------------------------------------------------------------
    // REQ-36: LeaseProvider bridge
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_identity_first_gateway_lease_provider_acquire() {
        let mock = Arc::new(MockBridge::new());
        let grant = LeaseGrant {
            identity: AgentIdentity::parse("agent:alpha").unwrap(),
            fencing_token: FencingToken::new(1),
            ttl: Duration::from_secs(30),
        };
        let mut response_map = BTreeMap::new();
        response_map.insert(
            AgentIdentity::parse("agent:alpha").unwrap(),
            LeaseAcquireResult::Acquired(grant),
        );
        mock.set_response(
            "callback/lease_provider/acquire_leases",
            Ok(serde_json::to_value(&response_map).unwrap()),
        )
        .await;

        let provider = GatewayLeaseProvider::new(mock.clone());
        let ids = vec![AgentIdentity::parse("agent:alpha").unwrap()];
        let result = provider.acquire_leases(&ids, "runtime-1").await.unwrap();
        assert_eq!(result.len(), 1);

        // Verify wire format
        let (_, params) = mock.last_call().await;
        assert_eq!(params["identities"][0].as_str().unwrap(), "agent:alpha");
        assert_eq!(params["runtime_instance"].as_str().unwrap(), "runtime-1");

        // Verify result deserialization
        let id = AgentIdentity::parse("agent:alpha").unwrap();
        match &result[&id] {
            LeaseAcquireResult::Acquired(g) => {
                assert_eq!(g.fencing_token, FencingToken::new(1));
                assert_eq!(g.ttl, Duration::from_secs(30));
            }
            other => panic!("expected Acquired, got: {other:?}"),
        }
    }

    #[tokio::test]
    async fn test_identity_first_gateway_lease_provider_renew() {
        let mock = Arc::new(MockBridge::new());
        let renewed_grant = LeaseGrant {
            identity: AgentIdentity::parse("agent:alpha").unwrap(),
            fencing_token: FencingToken::new(1),
            ttl: Duration::from_secs(30),
        };
        let mut response_map = BTreeMap::new();
        response_map.insert(
            AgentIdentity::parse("agent:alpha").unwrap(),
            LeaseRenewResult::Renewed(renewed_grant.clone()),
        );
        mock.set_response(
            "callback/lease_provider/renew_leases",
            Ok(serde_json::to_value(&response_map).unwrap()),
        )
        .await;

        let provider = GatewayLeaseProvider::new(mock.clone());
        let grants = vec![renewed_grant];
        let result = provider.renew_leases(&grants).await.unwrap();
        assert_eq!(result.len(), 1);

        // Verify grants were serialized with ttl as ms
        let (_, params) = mock.last_call().await;
        let sent_grants = params["grants"].as_array().unwrap();
        assert_eq!(sent_grants[0]["ttl"].as_u64().unwrap(), 30000);
    }

    #[tokio::test]
    async fn test_identity_first_gateway_lease_provider_release() {
        let mock = Arc::new(MockBridge::new());
        mock.set_response("callback/lease_provider/release_leases", Ok(Value::Null))
            .await;

        let provider = GatewayLeaseProvider::new(mock.clone());
        let grants = vec![LeaseGrant {
            identity: AgentIdentity::parse("agent:alpha").unwrap(),
            fencing_token: FencingToken::new(5),
            ttl: Duration::from_secs(10),
        }];
        provider.release_leases(&grants).await.unwrap();

        let (method, _) = mock.last_call().await;
        assert_eq!(method, "callback/lease_provider/release_leases");
    }

    // -----------------------------------------------------------------------
    // REQ-37: RosterProvider bridge
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_identity_first_gateway_roster_provider_roster() {
        let mock = Arc::new(MockBridge::new());
        mock.set_response(
            "callback/roster_provider/roster",
            Ok(json!([
                {
                    "identity": "triage:main",
                    "profile": "default",
                    "addressability": "addressable",
                    "display_name": "Triage",
                    "labels": { "role": "triage" },
                    "context": { "key": "value" },
                    "additional_instructions": ["Be helpful."],
                    "runtime_mode_override": "turn_driven",
                    "backend": "external",
                    "binding": {
                        "kind": "external",
                        "address": "tcp://127.0.0.1:4777",
                        "identity": {
                            "kind": "ed25519_public_key",
                            "public_key": "ed25519:BwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwcHBwc="
                        }
                    }
                }
            ])),
        )
        .await;

        let provider = GatewayRosterProvider::new(mock.clone());
        let ctx = RosterContext {
            mob_definition: None,
            previous_identities: vec![AgentIdentity::parse("old:agent").unwrap()],
        };
        let result = provider.roster(&ctx).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].identity.as_str(), "triage:main");
        assert_eq!(result[0].display_name.as_ref().unwrap().as_str(), "Triage");
        assert_eq!(result[0].labels["role"], "triage");
        assert_eq!(result[0].additional_instructions[0], "Be helpful.");
        assert_eq!(
            result[0].runtime_mode_override,
            Some(meerkat_mob::MobRuntimeMode::TurnDriven)
        );
        assert_eq!(
            result[0].backend,
            Some(meerkat_mob::MobBackendKind::External)
        );
        assert!(matches!(
            result[0].binding,
            Some(meerkat_contracts::WireRuntimeBinding::External { .. })
        ));

        // Verify previous_identities was sent
        let (_, params) = mock.last_call().await;
        let prev = params["previous_identities"].as_array().unwrap();
        assert_eq!(prev[0].as_str().unwrap(), "old:agent");
    }

    // -----------------------------------------------------------------------
    // REQ-38: AgentCustomizer bridge
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_identity_first_gateway_customizer_customize_build() {
        let mock = Arc::new(MockBridge::new());

        // The SDK returns the mutated draft
        let returned_draft = AgentBuildDraft {
            model: Some("gpt-5".to_string()),
            system_prompt: Some("You are a helpful agent.".to_string()),
            additional_instructions: vec!["Be concise.".to_string()],
            labels: {
                let mut m = BTreeMap::new();
                m.insert("custom".to_string(), "label".to_string());
                m
            },
            app_context: Some(json!({"injected": true})),
            external_tools: vec![ExternalToolDef {
                name: "my_tool".to_string(),
                description: "A custom tool".to_string(),
                input_schema: json!({"type": "object", "properties": {"x": {"type": "string"}}}),
            }],
            local_external_tools: Default::default(),
        };
        mock.set_response(
            "callback/agent_customizer/customize_build",
            Ok(serde_json::to_value(&returned_draft).unwrap()),
        )
        .await;

        let customizer = GatewayAgentCustomizer::new(mock.clone());
        let id = AgentIdentity::parse("agent:alpha").unwrap();
        let context = AgentBuildContext {
            identity: id.clone(),
            active_peers: vec![AgentIdentity::parse("agent:beta").unwrap()],
            managed_edges: vec![
                ManagedPeerEdge::new(
                    AgentIdentity::parse("agent:alpha").unwrap(),
                    AgentIdentity::parse("agent:beta").unwrap(),
                )
                .unwrap(),
            ],
            runtime_services: Default::default(),
        };
        let spec = DurableAgentSpec {
            identity: id,
            profile: meerkat_mob::ProfileName::from("default"),
            addressability: AgentAddressability::Addressable,
            display_name: None,
            labels: BTreeMap::new(),
            context: None,
            additional_instructions: vec![],
            initial_message: None,
            runtime_mode_override: None,
            backend: None,
            binding: None,
        };
        let mut draft = AgentBuildDraft {
            model: None,
            system_prompt: None,
            additional_instructions: vec![],
            labels: BTreeMap::new(),
            app_context: None,
            external_tools: vec![],
            local_external_tools: Default::default(),
        };

        customizer
            .customize_build(&context, &spec, &mut draft)
            .await
            .unwrap();

        // Verify draft was mutated
        assert_eq!(draft.model.as_deref(), Some("gpt-5"));
        assert_eq!(
            draft.system_prompt.as_deref(),
            Some("You are a helpful agent.")
        );
        assert_eq!(draft.additional_instructions, vec!["Be concise."]);
        assert_eq!(draft.labels["custom"], "label");
        assert_eq!(draft.external_tools.len(), 1);
        assert_eq!(draft.external_tools[0].name, "my_tool");
        let dispatcher = draft
            .local_external_tools
            .dispatcher()
            .expect("SDK external_tools should install a local callback dispatcher");
        assert_eq!(dispatcher.tools().len(), 1);
        assert_eq!(dispatcher.tools()[0].name.as_ref(), "my_tool");

        // Verify wire format sent to SDK
        let (method, params) = mock.last_call().await;
        assert_eq!(method, "callback/agent_customizer/customize_build");
        // context.active_peers as string array
        let peers = params["context"]["active_peers"].as_array().unwrap();
        assert_eq!(peers[0].as_str().unwrap(), "agent:beta");
        // context.managed_edges as array of {a, b}
        let edges = params["context"]["managed_edges"].as_array().unwrap();
        assert!(edges[0]["a"].is_string());
        assert!(edges[0]["b"].is_string());
        // spec.identity as string
        assert_eq!(params["spec"]["identity"].as_str().unwrap(), "agent:alpha");
        // draft does NOT include resume_session
        assert!(params["draft"].get("resume_session").is_none());
    }

    #[tokio::test]
    async fn test_identity_first_gateway_customizer_after_create() {
        let mock = Arc::new(MockBridge::new());
        mock.set_response("callback/agent_customizer/after_create", Ok(Value::Null))
            .await;

        let customizer = GatewayAgentCustomizer::new(mock.clone());
        let id = AgentIdentity::parse("agent:alpha").unwrap();
        let sid = meerkat_core::types::SessionId::new();
        let ctx = SessionCreatedContext {
            model: "gpt-5".to_string(),
            labels: {
                let mut m = BTreeMap::new();
                m.insert("key".to_string(), "value".to_string());
                m
            },
            system_prompt: Some("You are helpful.".to_string()),
        };
        customizer.after_create(&id, &sid, &ctx).await.unwrap();

        let (method, params) = mock.last_call().await;
        assert_eq!(method, "callback/agent_customizer/after_create");
        assert_eq!(params["identity"].as_str().unwrap(), "agent:alpha");
        assert_eq!(params["session_id"].as_str().unwrap(), sid.to_string());
        assert_eq!(params["model"].as_str().unwrap(), "gpt-5");
    }

    // -----------------------------------------------------------------------
    // REQ-39: TopologyProvider bridge
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_identity_first_gateway_topology_provider_compute_edges() {
        let mock = Arc::new(MockBridge::new());
        let edges = vec![
            ManagedPeerEdge::new(
                AgentIdentity::parse("agent:alpha").unwrap(),
                AgentIdentity::parse("agent:beta").unwrap(),
            )
            .unwrap(),
        ];
        mock.set_response(
            "callback/topology_provider/compute_edges",
            Ok(serde_json::to_value(&edges).unwrap()),
        )
        .await;

        let provider = GatewayTopologyProvider::new(mock.clone());
        let ids = vec![
            AgentIdentity::parse("agent:alpha").unwrap(),
            AgentIdentity::parse("agent:beta").unwrap(),
        ];
        let roster_spec = DurableAgentSpec {
            identity: AgentIdentity::parse("agent:alpha").unwrap(),
            profile: meerkat_mob::ProfileName::from("default"),
            addressability: AgentAddressability::Addressable,
            display_name: None,
            labels: BTreeMap::new(),
            context: None,
            additional_instructions: vec![],
            initial_message: None,
            runtime_mode_override: None,
            backend: None,
            binding: None,
        };
        let ctx = TopologyContext {
            roster: vec![roster_spec],
        };
        let result = provider.compute_edges(&ids, &ctx).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].a().as_str(), "agent:alpha");
        assert_eq!(result[0].b().as_str(), "agent:beta");

        // Verify wire format
        let (_, params) = mock.last_call().await;
        let sent_ids = params["target_identities"].as_array().unwrap();
        assert_eq!(sent_ids[0].as_str().unwrap(), "agent:alpha");
        assert_eq!(sent_ids[1].as_str().unwrap(), "agent:beta");
        // context.roster should be serialized
        assert!(params["context"]["roster"].is_array());
    }

    #[tokio::test]
    async fn test_identity_first_gateway_topology_empty_edges() {
        let mock = Arc::new(MockBridge::new());
        mock.set_response("callback/topology_provider/compute_edges", Ok(json!([])))
            .await;

        let provider = GatewayTopologyProvider::new(mock.clone());
        let ids = vec![AgentIdentity::parse("agent:solo").unwrap()];
        let ctx = TopologyContext { roster: vec![] };
        let result = provider.compute_edges(&ids, &ctx).await.unwrap();
        assert!(result.is_empty());
    }

    // -----------------------------------------------------------------------
    // Wire format round-trip tests (newtypes serialize correctly)
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_identity_first_gateway_newtype_wire_format() {
        // Verify that newtypes serialize to their expected wire representations
        // AgentIdentity → string
        let id = AgentIdentity::parse("triage:main").unwrap();
        let v = serde_json::to_value(&id).unwrap();
        assert_eq!(v.as_str().unwrap(), "triage:main");

        // FencingToken → integer
        let ft = FencingToken::new(42);
        let v = serde_json::to_value(ft).unwrap();
        assert_eq!(v.as_u64().unwrap(), 42);

        // CheckpointVersion → integer
        let cv = CheckpointVersion::new(7);
        let v = serde_json::to_value(cv).unwrap();
        assert_eq!(v.as_u64().unwrap(), 7);

        // ContinuityGeneration → integer
        let generation = ContinuityGeneration::new(3);
        let v = serde_json::to_value(generation).unwrap();
        assert_eq!(v.as_u64().unwrap(), 3);

        // LeaseGrant.ttl → integer milliseconds
        let grant = LeaseGrant {
            identity: id,
            fencing_token: ft,
            ttl: Duration::from_secs(15),
        };
        let v = serde_json::to_value(&grant).unwrap();
        assert_eq!(v["ttl"].as_u64().unwrap(), 15000);

        // SessionSnapshot → {"data": "<base64>"} per spec TYPE-22
        let snapshot = SessionSnapshot {
            data: b"hello world".to_vec(),
        };
        let v = serde_json::to_value(&snapshot).unwrap();
        assert!(v.is_object());
        assert!(v.get("data").and_then(|d| d.as_str()).is_some());
        let decoded: SessionSnapshot = serde_json::from_value(v).unwrap();
        assert_eq!(decoded.data, b"hello world");
    }
}
