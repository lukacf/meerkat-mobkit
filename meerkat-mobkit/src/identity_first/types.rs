use std::sync::Arc;

use serde::{Deserialize, Serialize};
use std::fmt;

// ---------------------------------------------------------------------------
// Validation helpers
// ---------------------------------------------------------------------------

/// Error returned when an identity string fails validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidIdentity {
    pub input: String,
    pub reason: String,
}

impl fmt::Display for InvalidIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid identity {:?}: {}", self.input, self.reason)
    }
}

impl std::error::Error for InvalidIdentity {}

fn validate_identity_string(s: &str) -> Result<(), InvalidIdentity> {
    if s.is_empty() {
        return Err(InvalidIdentity {
            input: s.to_string(),
            reason: "must not be empty".to_string(),
        });
    }
    if s.contains(char::is_whitespace) {
        return Err(InvalidIdentity {
            input: s.to_string(),
            reason: "must not contain whitespace".to_string(),
        });
    }
    if s.contains('/') {
        return Err(InvalidIdentity {
            input: s.to_string(),
            reason: "must not contain slashes".to_string(),
        });
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Macro for validated string newtypes (AgentIdentity, AgentRuntimeId)
// ---------------------------------------------------------------------------

macro_rules! validated_string_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(String);

        impl $name {
            /// Parse and validate a string into this type.
            ///
            /// # Errors
            ///
            /// Returns `InvalidIdentity` if the input is empty, contains whitespace,
            /// or contains slashes.
            pub fn parse(s: &str) -> Result<Self, InvalidIdentity> {
                validate_identity_string(s)?;
                Ok(Self(s.to_string()))
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                serializer.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let s = String::deserialize(deserializer)?;
                Self::parse(&s).map_err(serde::de::Error::custom)
            }
        }
    };
}

validated_string_newtype!(
    /// The primary app-facing identity handle for all MobKit control-plane operations.
    AgentIdentity
);

validated_string_newtype!(
    /// Internal runtime-level ID minted at first-create.
    AgentRuntimeId
);

// ---------------------------------------------------------------------------
// AgentAddressability
// ---------------------------------------------------------------------------

/// Whether an agent accepts `send()` (addressable) or only `dispatch()` (internal-only).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentAddressability {
    #[default]
    Addressable,
    InternalOnly,
}

// ---------------------------------------------------------------------------
// DisplayName
// ---------------------------------------------------------------------------

/// Human-facing display name. Non-empty.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DisplayName(String);

impl DisplayName {
    /// # Errors
    ///
    /// Returns `InvalidIdentity` if the input is empty.
    pub fn parse(s: &str) -> Result<Self, InvalidIdentity> {
        if s.is_empty() {
            return Err(InvalidIdentity {
                input: s.to_string(),
                reason: "display name must not be empty".to_string(),
            });
        }
        Ok(Self(s.to_string()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DisplayName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Serialize for DisplayName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for DisplayName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::parse(&s).map_err(serde::de::Error::custom)
    }
}

// ---------------------------------------------------------------------------
// Monotonic u64 newtypes
// ---------------------------------------------------------------------------

macro_rules! monotonic_u64_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(u64);

        impl $name {
            #[must_use]
            pub const fn new(value: u64) -> Self {
                Self(value)
            }

            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

monotonic_u64_newtype!(
    /// Monotonic generation counter for continuity. Starts at 0, incremented by `reset()`.
    ContinuityGeneration
);

monotonic_u64_newtype!(
    /// Monotonic checkpoint counter scoped to `(AgentIdentity, ContinuityGeneration)`.
    CheckpointVersion
);

monotonic_u64_newtype!(
    /// Monotonic ownership token issued by `LeaseProvider`.
    FencingToken
);

// ---------------------------------------------------------------------------
// Lightweight string newtypes (no validation beyond serde)
// ---------------------------------------------------------------------------

macro_rules! string_newtype {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            #[must_use]
            pub fn new(s: impl Into<String>) -> Self {
                Self(s.into())
            }

            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

string_newtype!(
    /// Correlation ID for dispatch tracing.
    CorrelationId
);

string_newtype!(
    /// Idempotency key for dispatch deduplication.
    DispatchIdempotencyKey
);

// ---------------------------------------------------------------------------
// ContinuityRecord
// ---------------------------------------------------------------------------

/// The authoritative continuity record for a durable agent identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuityRecord {
    pub identity: AgentIdentity,
    pub agent_runtime_id: AgentRuntimeId,
    pub session_id: meerkat_core::types::SessionId,
    pub generation: ContinuityGeneration,
    pub checkpoint_version: CheckpointVersion,
}

// ---------------------------------------------------------------------------
// ContinuityFailure + ContinuityFailureKind
// ---------------------------------------------------------------------------

/// Kind of continuity failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContinuityFailureKind {
    SnapshotMissing,
    SnapshotCorrupted,
    GenerationMismatch,
    StoreUnavailable,
}

/// A typed failure payload for broken continuity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuityFailure {
    pub identity: AgentIdentity,
    pub kind: ContinuityFailureKind,
    pub record: Option<ContinuityRecord>,
    pub detail: String,
}

// ---------------------------------------------------------------------------
// ContinuityResolveState
// ---------------------------------------------------------------------------

/// The resolve result for a single identity from the continuity store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum ContinuityResolveState {
    Uninitialized,
    Ready { record: ContinuityRecord },
    Broken { failure: ContinuityFailure },
}

// ---------------------------------------------------------------------------
// LeaseGrant
// ---------------------------------------------------------------------------

/// A lease grant returned by `LeaseProvider`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseGrant {
    pub identity: AgentIdentity,
    pub fencing_token: FencingToken,
    #[serde(
        serialize_with = "serde_duration_ms::serialize",
        deserialize_with = "serde_duration_ms::deserialize"
    )]
    pub ttl: std::time::Duration,
}

/// Custom serde for `Duration` as integer milliseconds.
mod serde_duration_ms {
    use serde::{Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let ms = duration.as_millis();
        // u128 -> u64 is safe for any reasonable TTL
        serializer.serialize_u64(ms as u64)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Duration, D::Error>
    where
        D: Deserializer<'de>,
    {
        let ms = u64::deserialize(deserializer)?;
        Ok(Duration::from_millis(ms))
    }
}

// ---------------------------------------------------------------------------
// LeaseAcquireResult + LeaseRenewResult
// ---------------------------------------------------------------------------

/// Result of a lease acquisition attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum LeaseAcquireResult {
    Acquired(LeaseGrant),
    AlreadyHeld {
        identity: AgentIdentity,
        holder: String,
    },
}

/// Result of a lease renewal attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum LeaseRenewResult {
    Renewed(LeaseGrant),
    Lost { identity: AgentIdentity },
}

// ---------------------------------------------------------------------------
// DispatchOrigin + DispatchInput
// ---------------------------------------------------------------------------

/// Origin of a dispatch request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DispatchOrigin {
    Connector,
    Scheduler,
    Policy,
    Flow,
    System,
}

/// Input for a dispatch operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DispatchInput {
    pub content: meerkat_core::ContentInput,
    pub origin: DispatchOrigin,
    pub correlation_id: Option<CorrelationId>,
    pub idempotency_key: Option<DispatchIdempotencyKey>,
}

impl DispatchInput {
    /// System-origin dispatch from plain text. The common case.
    pub fn system(text: impl Into<String>) -> Self {
        Self {
            content: meerkat_core::ContentInput::Text(text.into()),
            origin: DispatchOrigin::System,
            correlation_id: None,
            idempotency_key: None,
        }
    }

    /// Dispatch with an explicit origin from plain text.
    pub fn with_origin(text: impl Into<String>, origin: DispatchOrigin) -> Self {
        Self {
            content: meerkat_core::ContentInput::Text(text.into()),
            origin,
            correlation_id: None,
            idempotency_key: None,
        }
    }

    /// Attach a correlation ID (builder pattern).
    pub fn with_correlation(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(CorrelationId::new(id));
        self
    }

    /// Attach an idempotency key (builder pattern).
    pub fn with_idempotency(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(DispatchIdempotencyKey::new(key));
        self
    }
}

// ---------------------------------------------------------------------------
// ManagedPeerEdge
// ---------------------------------------------------------------------------

/// Error returned when constructing an invalid `ManagedPeerEdge`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManagedPeerEdgeError {
    SelfEdge,
}

impl fmt::Display for ManagedPeerEdgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SelfEdge => write!(f, "self-edges are not allowed"),
        }
    }
}

impl std::error::Error for ManagedPeerEdgeError {}

/// A managed dynamic topology edge between two agent identities.
///
/// Canonical ordering: `a < b`. Self-edges are rejected at construction time.
/// Deserialization enforces the same invariant as `new()` via `TryFrom`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(into = "ManagedPeerEdgeRaw")]
pub struct ManagedPeerEdge {
    a: AgentIdentity,
    b: AgentIdentity,
}

/// Raw deserialization target for `ManagedPeerEdge`.
#[derive(Serialize, Deserialize)]
struct ManagedPeerEdgeRaw {
    a: AgentIdentity,
    b: AgentIdentity,
}

impl From<ManagedPeerEdge> for ManagedPeerEdgeRaw {
    fn from(edge: ManagedPeerEdge) -> Self {
        Self {
            a: edge.a,
            b: edge.b,
        }
    }
}

impl TryFrom<ManagedPeerEdgeRaw> for ManagedPeerEdge {
    type Error = ManagedPeerEdgeError;

    fn try_from(raw: ManagedPeerEdgeRaw) -> Result<Self, Self::Error> {
        Self::new(raw.a, raw.b)
    }
}

impl<'de> Deserialize<'de> for ManagedPeerEdge {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = ManagedPeerEdgeRaw::deserialize(deserializer)?;
        Self::try_from(raw).map_err(serde::de::Error::custom)
    }
}

impl ManagedPeerEdge {
    /// Construct a managed peer edge with canonical ordering enforcement.
    ///
    /// # Errors
    ///
    /// Returns `ManagedPeerEdgeError::SelfEdge` if `a == b`.
    pub fn new(a: AgentIdentity, b: AgentIdentity) -> Result<Self, ManagedPeerEdgeError> {
        if a == b {
            return Err(ManagedPeerEdgeError::SelfEdge);
        }
        if a < b {
            Ok(Self { a, b })
        } else {
            Ok(Self { a: b, b: a })
        }
    }

    #[must_use]
    pub fn a(&self) -> &AgentIdentity {
        &self.a
    }

    #[must_use]
    pub fn b(&self) -> &AgentIdentity {
        &self.b
    }
}

// ---------------------------------------------------------------------------
// NotAddressable error
// ---------------------------------------------------------------------------

/// Error returned when `send()` targets an `InternalOnly` agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotAddressable {
    pub identity: AgentIdentity,
    pub addressability: AgentAddressability,
}

impl fmt::Display for NotAddressable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "agent {:?} is not addressable (current: {:?})",
            self.identity, self.addressability
        )
    }
}

impl std::error::Error for NotAddressable {}

// ---------------------------------------------------------------------------
// DurableAgentSpec
// ---------------------------------------------------------------------------

/// The preferred roster/spawn specification for identity-first continuity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DurableAgentSpec {
    pub identity: AgentIdentity,
    pub profile: meerkat_mob::ProfileName,
    #[serde(default)]
    pub addressability: AgentAddressability,
    pub display_name: Option<DisplayName>,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
    pub context: Option<serde_json::Value>,
    #[serde(default)]
    pub additional_instructions: Vec<String>,
    #[serde(default)]
    pub initial_message: Option<meerkat_core::ContentInput>,
    #[serde(default)]
    pub runtime_mode_override: Option<meerkat_mob::MobRuntimeMode>,
}

// ---------------------------------------------------------------------------
// IdentityStatus + supporting types
// ---------------------------------------------------------------------------

/// Lifecycle state of an identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityLifecycleState {
    Active,
    Retiring,
    Suspended,
    Uninitialized,
}

/// Information about a held lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseInfo {
    pub fencing_token: FencingToken,
    #[serde(
        serialize_with = "serde_duration_ms::serialize",
        deserialize_with = "serde_duration_ms::deserialize"
    )]
    pub ttl_remaining: std::time::Duration,
    pub healthy: bool,
}

/// Durability policy declared by the continuity store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum DurabilityPolicy {
    SyncWriteThrough,
    AsyncReplicated,
    BufferedExport { max_loss_window_ms: u64 },
}

/// Health of the continuity store for an identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContinuityHealth {
    pub store_reachable: bool,
    pub durability_policy: DurabilityPolicy,
    pub last_checkpoint_version: Option<CheckpointVersion>,
}

/// Full status response for an identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IdentityStatus {
    pub identity: AgentIdentity,
    pub state: IdentityLifecycleState,
    pub agent_runtime_id: Option<AgentRuntimeId>,
    pub session_id: Option<meerkat_core::types::SessionId>,
    pub profile: Option<meerkat_mob::ProfileName>,
    pub runtime_mode: Option<meerkat_mob::MobRuntimeMode>,
    pub addressability: AgentAddressability,
    pub display_name: Option<DisplayName>,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
    pub generation: Option<ContinuityGeneration>,
    pub checkpoint_version: Option<CheckpointVersion>,
    pub lease: Option<LeaseInfo>,
    pub continuity_health: Option<ContinuityHealth>,
}

// ---------------------------------------------------------------------------
// AgentBuildContext + AgentBuildDraft + ExternalToolDef
// ---------------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct AgentRuntimeServices {
    mob_handle: Option<meerkat_mob::MobHandle>,
}

impl AgentRuntimeServices {
    pub fn new(mob_handle: meerkat_mob::MobHandle) -> Self {
        Self {
            mob_handle: Some(mob_handle),
        }
    }

    pub fn empty() -> Self {
        Self { mob_handle: None }
    }

    pub fn mob_handle(&self) -> Option<meerkat_mob::MobHandle> {
        self.mob_handle.clone()
    }

    pub fn has_mob_handle(&self) -> bool {
        self.mob_handle.is_some()
    }
}

impl std::fmt::Debug for AgentRuntimeServices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentRuntimeServices")
            .field("mob_handle", &self.mob_handle.is_some())
            .finish()
    }
}

impl PartialEq for AgentRuntimeServices {
    fn eq(&self, other: &Self) -> bool {
        self.mob_handle.is_some() == other.mob_handle.is_some()
    }
}

impl Eq for AgentRuntimeServices {}

/// Read-only context provided to `AgentCustomizer` at build time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentBuildContext {
    pub identity: AgentIdentity,
    pub active_peers: Vec<AgentIdentity>,
    pub managed_edges: Vec<ManagedPeerEdge>,
    #[serde(default, skip)]
    pub runtime_services: AgentRuntimeServices,
}

/// Tool definition for the customizer boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// Mutable draft that `AgentCustomizer` modifies.
#[derive(Clone, Default)]
pub struct LocalExternalToolOverlay {
    dispatcher: Option<Arc<dyn meerkat_core::agent::AgentToolDispatcher>>,
}

impl LocalExternalToolOverlay {
    pub fn new(dispatcher: Arc<dyn meerkat_core::agent::AgentToolDispatcher>) -> Self {
        Self {
            dispatcher: Some(dispatcher),
        }
    }

    pub fn empty() -> Self {
        Self { dispatcher: None }
    }

    pub fn dispatcher(&self) -> Option<Arc<dyn meerkat_core::agent::AgentToolDispatcher>> {
        self.dispatcher.clone()
    }

    pub fn is_some(&self) -> bool {
        self.dispatcher.is_some()
    }
}

impl std::fmt::Debug for LocalExternalToolOverlay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalExternalToolOverlay")
            .field("dispatcher", &self.dispatcher.is_some())
            .finish()
    }
}

impl PartialEq for LocalExternalToolOverlay {
    fn eq(&self, other: &Self) -> bool {
        self.dispatcher.is_some() == other.dispatcher.is_some()
    }
}

impl Eq for LocalExternalToolOverlay {}

/// Mutable draft that `AgentCustomizer` modifies.
///
/// `external_tools` remains the serializable SDK/gateway declaration surface.
/// `local_external_tools` is intentionally skipped by serde and is the
/// in-process Rust overlay for apps that can supply a real dispatcher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentBuildDraft {
    pub model: Option<String>,
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub additional_instructions: Vec<String>,
    #[serde(default)]
    pub labels: std::collections::BTreeMap<String, String>,
    pub app_context: Option<serde_json::Value>,
    #[serde(default)]
    pub external_tools: Vec<ExternalToolDef>,
    #[serde(default, skip)]
    pub local_external_tools: LocalExternalToolOverlay,
}

// ---------------------------------------------------------------------------
// SessionSnapshot
// ---------------------------------------------------------------------------

/// Opaque wrapper around serialized Meerkat session state.
///
/// Stored and loaded by `ContinuityStore`.
/// Wire format (JSON-RPC): `{ "data": "<base64 string>" }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub data: Vec<u8>,
}

impl Serialize for SessionSnapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use base64::Engine;
        use serde::ser::SerializeStruct;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&self.data);
        let mut s = serializer.serialize_struct("SessionSnapshot", 1)?;
        s.serialize_field("data", &encoded)?;
        s.end()
    }
}

impl<'de> Deserialize<'de> for SessionSnapshot {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use base64::Engine;

        #[derive(Deserialize)]
        struct Wrapper {
            data: String,
        }

        let wrapper = Wrapper::deserialize(deserializer)?;
        let data = base64::engine::general_purpose::STANDARD
            .decode(&wrapper.data)
            .map_err(serde::de::Error::custom)?;
        Ok(Self { data })
    }
}

// ---------------------------------------------------------------------------
// RosterContext + TopologyContext
// ---------------------------------------------------------------------------

/// Context passed to `RosterProvider`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RosterContext {
    pub mob_definition: Option<meerkat_mob::MobDefinition>,
    pub previous_identities: Vec<AgentIdentity>,
}

/// Context passed to `TopologyProvider`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TopologyContext {
    pub roster: Vec<DurableAgentSpec>,
}

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Error from the continuity store.
#[derive(Debug)]
pub enum ContinuityStoreError {
    StaleFencingToken {
        identity: AgentIdentity,
        presented: FencingToken,
        current: FencingToken,
    },
    StaleCheckpointVersion {
        identity: AgentIdentity,
        presented: CheckpointVersion,
        current: CheckpointVersion,
    },
    NotFound {
        identity: AgentIdentity,
    },
    Io(String),
    Corruption(String),
}

impl fmt::Display for ContinuityStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StaleFencingToken {
                identity,
                presented,
                current,
            } => write!(
                f,
                "stale fencing token for {identity}: presented {presented}, current {current}"
            ),
            Self::StaleCheckpointVersion {
                identity,
                presented,
                current,
            } => write!(
                f,
                "stale checkpoint version for {identity}: presented {presented}, current {current}"
            ),
            Self::NotFound { identity } => {
                write!(f, "continuity record not found for {identity}")
            }
            Self::Io(msg) => write!(f, "continuity store I/O error: {msg}"),
            Self::Corruption(msg) => write!(f, "continuity store corruption: {msg}"),
        }
    }
}

impl std::error::Error for ContinuityStoreError {}

/// Error from the lease provider.
#[derive(Debug)]
pub enum LeaseError {
    ProviderUnavailable(String),
    Io(String),
}

impl fmt::Display for LeaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProviderUnavailable(msg) => {
                write!(f, "lease provider unavailable: {msg}")
            }
            Self::Io(msg) => write!(f, "lease I/O error: {msg}"),
        }
    }
}

impl std::error::Error for LeaseError {}

/// Error from the roster provider.
#[derive(Debug)]
pub enum RosterError {
    ProviderUnavailable(String),
    Io(String),
}

impl fmt::Display for RosterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProviderUnavailable(msg) => {
                write!(f, "roster provider unavailable: {msg}")
            }
            Self::Io(msg) => write!(f, "roster I/O error: {msg}"),
        }
    }
}

impl std::error::Error for RosterError {}

/// Error from the topology provider.
#[derive(Debug)]
pub enum TopologyError {
    InvalidEdge(String),
    ProviderUnavailable(String),
}

impl fmt::Display for TopologyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidEdge(msg) => write!(f, "invalid topology edge: {msg}"),
            Self::ProviderUnavailable(msg) => {
                write!(f, "topology provider unavailable: {msg}")
            }
        }
    }
}

impl std::error::Error for TopologyError {}

/// Error from the agent customizer.
#[derive(Debug)]
pub enum CustomizerError {
    BuildFailed(String),
    Io(String),
}

impl fmt::Display for CustomizerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BuildFailed(msg) => write!(f, "customizer build failed: {msg}"),
            Self::Io(msg) => write!(f, "customizer I/O error: {msg}"),
        }
    }
}

impl std::error::Error for CustomizerError {}
