//! Cross-mob control protocol: the control-plane RPC that crosses
//! processes.
//!
//! * `ControlRequest` / `ControlResponse` - the on-the-wire types.
//! * [`serve_tcp_control`] / [`serve_uds_control`] - accept connections on
//!   a gateway's control listener, read framed requests, dispatch them
//!   against a local mob via a [`ControlHandler`], and write back framed
//!   responses. `UnifiedRuntime::start_control_listener` binds the
//!   listener and spawns the serve task.
//! * [`RemoteControlClient`] - opens a connection per request, sends a
//!   single frame, reads the response. Used by
//!   [`super::cross_mob_remote::RemoteMobProxy`].
//!
//! # Wire shape
//!
//! Each frame is `[u32 BE length][JSON UTF-8 payload]`. JSON is preferred
//! over CBOR for control because it stays human-debuggable in pcap traces
//! and the volume is tiny (one message per `wire`/`unwire`/`inject` call,
//! not per agent turn). `meerkat-comms`'s `TransportCodec` is reserved for
//! agent envelopes.
//!
//! # Trust
//!
//! The control listener does NOT verify Ed25519 signatures itself. Trust
//! is delegated to the contact directory: when a peer mobkit calls
//! `RemoteMobProxy::wire_remote`, it includes the *peer's* expected
//! `peer_pubkey_b64` in the request. The remote gateway feeds that pubkey
//! into the resulting `TrustedPeerDescriptor`, and `meerkat-comms` rejects
//! envelope traffic that doesn't match it. So the control channel itself
//! is unauthenticated, but the artifacts it produces (peer descriptors)
//! are signature-checked at every subsequent comms ingress.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

use super::cross_mob_remote::{RemoteEndpoint, RemoteMobError};

/// Maximum control payload size. Control messages are tiny (a few hundred
/// bytes typical, ~1 KiB max for an injected text turn) — we cap well below
/// that so a misbehaving peer can't tie up reads.
const MAX_CONTROL_PAYLOAD: u32 = 64 * 1024;

/// Default request timeout. Control RPC should be fast — local mob
/// dispatch on the remote side is sub-millisecond. We give it 5s to absorb
/// scheduler pauses, network latency, and lazy-spawn warm-up.
pub const DEFAULT_CONTROL_TIMEOUT: Duration = Duration::from_secs(5);

/// Cross-mob control request. One variant per control operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ControlRequest {
    /// Wire a local member to a peer mob's member.
    Wire {
        /// Member identity in the *receiving* (remote) mob.
        remote_member: String,
        /// Peer descriptor of the *calling* gateway's local member.
        local_peer_spec_address: String,
        local_comms_name: String,
        local_peer_id: String,
        /// Optional Ed25519 pubkey of the calling gateway. When present,
        /// the receiving gateway builds a signed `TrustedPeerDescriptor`
        /// so meerkat-comms can verify envelope signatures.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        local_pubkey_b64: Option<String>,
    },
    /// Unwire a previously-wired peer.
    Unwire {
        remote_member: String,
        local_peer_spec_address: String,
        local_comms_name: String,
        local_peer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        local_pubkey_b64: Option<String>,
    },
    /// Inject an external-turn message into a remote member's session.
    /// Used by `send_cross_mob` for app-level injection.
    Inject {
        remote_member: String,
        /// JSON-encoded `meerkat_core::ContentInput`.
        content: serde_json::Value,
    },
    /// Look up a member's comms info on the remote side. Used during
    /// `wire_cross_mob` to discover the remote member's peer_id and
    /// derived comms_name without requiring caller-supplied bookkeeping.
    LookupMember { remote_member: String },
}

/// Cross-mob control response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ControlResponse {
    /// Operation succeeded (no payload).
    Ok,
    /// Inject succeeded — return the bridge session id that accepted the
    /// injection so the caller can correlate downstream events.
    Injected { session_id: String },
    /// LookupMember succeeded — return the remote member's peer info so
    /// the caller can build a `TrustedPeerDescriptor` pointing at it.
    Member { peer_id: String, comms_name: String },
    /// Operation failed. `code` is a stable short string for machine
    /// dispatch (`unknown_member`, `mob_error`, `decode`, ...); `message`
    /// is human-readable.
    Err { code: String, message: String },
}

/// Open a control connection to a remote endpoint.
///
/// TCP and UDS share the same length-prefixed JSON framing, so we wrap
/// the underlying `TcpStream` / `UnixStream` in a small enum and write
/// codec-agnostic helpers below.
enum ControlStream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Uds(UnixStream),
}

impl ControlStream {
    async fn write_frame(&mut self, payload: &[u8]) -> Result<(), std::io::Error> {
        let len = u32::try_from(payload.len()).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "payload too large")
        })?;
        let header = len.to_be_bytes();
        match self {
            Self::Tcp(s) => {
                s.write_all(&header).await?;
                s.write_all(payload).await?;
                s.flush().await
            }
            #[cfg(unix)]
            Self::Uds(s) => {
                s.write_all(&header).await?;
                s.write_all(payload).await?;
                s.flush().await
            }
        }
    }

    async fn read_frame(&mut self) -> Result<Vec<u8>, std::io::Error> {
        let mut header = [0u8; 4];
        match self {
            Self::Tcp(s) => s.read_exact(&mut header).await?,
            #[cfg(unix)]
            Self::Uds(s) => s.read_exact(&mut header).await?,
        };
        let len = u32::from_be_bytes(header);
        if len > MAX_CONTROL_PAYLOAD {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("frame too large: {len} bytes"),
            ));
        }
        let mut buf = vec![0u8; len as usize];
        match self {
            Self::Tcp(s) => s.read_exact(&mut buf).await?,
            #[cfg(unix)]
            Self::Uds(s) => s.read_exact(&mut buf).await?,
        };
        Ok(buf)
    }
}

/// Client side of the cross-mob control protocol.
pub struct RemoteControlClient;

impl RemoteControlClient {
    /// Send `request` to `endpoint`, await one response, and return it.
    ///
    /// Opens a fresh connection per request - control RPC frequency is
    /// low (one message per wire/unwire/inject call) and lazy-reconnect
    /// keeps the implementation simple. Connection pooling can come later
    /// if profiling ever shows it matters.
    pub async fn send(
        endpoint: &RemoteEndpoint,
        request: &ControlRequest,
        timeout: Duration,
    ) -> Result<ControlResponse, RemoteMobError> {
        tokio::time::timeout(timeout, Self::send_inner(endpoint, request))
            .await
            .map_err(|_| RemoteMobError::ControlChannelUnavailable {
                mob_id: String::new(),
                endpoint: endpoint.comms_address(),
                operation: "timeout",
            })?
    }

    async fn send_inner(
        endpoint: &RemoteEndpoint,
        request: &ControlRequest,
    ) -> Result<ControlResponse, RemoteMobError> {
        let mut stream = match endpoint {
            RemoteEndpoint::Tcp(addr) => ControlStream::Tcp(
                TcpStream::connect(addr)
                    .await
                    .map_err(|err| io_error("connect", endpoint, err))?,
            ),
            #[cfg(unix)]
            RemoteEndpoint::Uds(path) => ControlStream::Uds(
                UnixStream::connect(std::path::Path::new(path))
                    .await
                    .map_err(|err| io_error("connect", endpoint, err))?,
            ),
            #[cfg(not(unix))]
            RemoteEndpoint::Uds(_) => {
                return Err(RemoteMobError::UnsupportedTransport {
                    mob_id: String::new(),
                    transport: endpoint.comms_address(),
                });
            }
        };
        let payload =
            serde_json::to_vec(request).map_err(|err| encode_error(endpoint, err.to_string()))?;
        stream
            .write_frame(&payload)
            .await
            .map_err(|err| io_error("write", endpoint, err))?;
        let response_payload = stream
            .read_frame()
            .await
            .map_err(|err| io_error("read", endpoint, err))?;
        serde_json::from_slice::<ControlResponse>(&response_payload)
            .map_err(|err| decode_error(endpoint, err.to_string()))
    }
}

fn io_error(stage: &'static str, endpoint: &RemoteEndpoint, err: std::io::Error) -> RemoteMobError {
    RemoteMobError::ControlChannelUnavailable {
        mob_id: String::new(),
        endpoint: endpoint.comms_address(),
        operation: match stage {
            "connect" => "connect",
            "write" => "write",
            "read" => "read",
            _ => "io",
        },
    }
    .with_context(err.to_string())
}

fn encode_error(endpoint: &RemoteEndpoint, message: String) -> RemoteMobError {
    RemoteMobError::Encode {
        endpoint: endpoint.comms_address(),
        message,
    }
}

fn decode_error(endpoint: &RemoteEndpoint, message: String) -> RemoteMobError {
    RemoteMobError::Decode {
        endpoint: endpoint.comms_address(),
        message,
    }
}

/// Asynchronously dispatch a single decoded `ControlRequest` against a
/// local mob handle, returning the response shape.
///
/// Boxed-future trait so we can store this in a `dyn` field without
/// requiring `async-trait` and so the listener doesn't need a generic
/// type parameter for the handler.
pub trait ControlHandler: Send + Sync + 'static {
    fn handle(
        &self,
        request: ControlRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ControlResponse> + Send + '_>>;
}

/// Real `ControlHandler` that dispatches requests against a local
/// `MobHandle`. Constructed by `UnifiedRuntime::start_control_listener`
/// when a control listener is configured (builder `control_listen()` or
/// the mobkit_gateway `--control-listen` flag).
pub struct MobHandleControlHandler {
    handle: meerkat_mob::MobHandle,
    identity_runtime: Option<std::sync::Arc<crate::identity_first::IdentityRuntime>>,
}

impl MobHandleControlHandler {
    /// Construct a classic member-plane handler. Reserved generated aliases
    /// fail closed because no durable identity authority was supplied.
    pub fn new(handle: meerkat_mob::MobHandle) -> Self {
        Self {
            handle,
            identity_runtime: None,
        }
    }

    /// Construct an identity-aware handler that pins every generated-alias
    /// operation to its current durable generation.
    pub fn with_identity_runtime(
        handle: meerkat_mob::MobHandle,
        identity_runtime: std::sync::Arc<crate::identity_first::IdentityRuntime>,
    ) -> Self {
        Self {
            handle,
            identity_runtime: Some(identity_runtime),
        }
    }
}

impl ControlHandler for MobHandleControlHandler {
    fn handle(
        &self,
        request: ControlRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ControlResponse> + Send + '_>> {
        let handle = self.handle.clone();
        let identity_runtime = self.identity_runtime.clone();
        Box::pin(async move {
            match request {
                ControlRequest::Wire {
                    remote_member,
                    local_peer_spec_address,
                    local_comms_name,
                    local_peer_id,
                    local_pubkey_b64,
                } => {
                    handle_wire(
                        &handle,
                        WireControlParams {
                            remote_member: &remote_member,
                            local_peer_spec_address: &local_peer_spec_address,
                            local_comms_name: &local_comms_name,
                            local_peer_id: &local_peer_id,
                            local_pubkey_b64: local_pubkey_b64.as_deref(),
                            wire: true,
                        },
                        identity_runtime.as_ref(),
                    )
                    .await
                }
                ControlRequest::Unwire {
                    remote_member,
                    local_peer_spec_address,
                    local_comms_name,
                    local_peer_id,
                    local_pubkey_b64,
                } => {
                    handle_wire(
                        &handle,
                        WireControlParams {
                            remote_member: &remote_member,
                            local_peer_spec_address: &local_peer_spec_address,
                            local_comms_name: &local_comms_name,
                            local_peer_id: &local_peer_id,
                            local_pubkey_b64: local_pubkey_b64.as_deref(),
                            wire: false,
                        },
                        identity_runtime.as_ref(),
                    )
                    .await
                }
                ControlRequest::Inject {
                    remote_member,
                    content,
                } => {
                    handle_inject(&handle, &remote_member, content, identity_runtime.as_ref()).await
                }
                ControlRequest::LookupMember { remote_member } => {
                    handle_lookup_member(&handle, &remote_member, identity_runtime.as_ref()).await
                }
            }
        })
    }
}

fn control_identity_error(error: crate::identity_first::IdentityRuntimeError) -> (String, String) {
    let code = match error {
        crate::identity_first::IdentityRuntimeError::StaleRuntimeAlias { .. } => {
            "stale_runtime_alias"
        }
        crate::identity_first::IdentityRuntimeError::UnknownIdentity(_) => "unknown_identity",
        crate::identity_first::IdentityRuntimeError::InvalidState { .. } => "identity_not_active",
        _ => "identity_error",
    };
    (code.to_string(), error.to_string())
}

const TRACKED_CONTROL_ERROR_PREFIX: &str = "mobkit-control-operation:";

fn tracked_control_error(error: crate::identity_first::IdentityRuntimeError) -> (String, String) {
    if let crate::identity_first::IdentityRuntimeError::Internal(message) = &error
        && let Some(encoded) = message.strip_prefix(TRACKED_CONTROL_ERROR_PREFIX)
        && let Ok((code, message)) = serde_json::from_str::<(String, String)>(encoded)
    {
        return (code, message);
    }
    control_identity_error(error)
}

async fn run_control_member_operation<T, F, Fut>(
    identity_runtime: Option<&std::sync::Arc<crate::identity_first::IdentityRuntime>>,
    member_alias: &str,
    operation: F,
) -> Result<T, (String, String)>
where
    T: Send + 'static,
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<T, (String, String)>> + Send + 'static,
{
    let member_alias = crate::member_comms_id::runtime_alias_str(member_alias).into_owned();
    if let Some(runtime) = identity_runtime {
        let target = runtime
            .member_alias_lifecycle_target(&member_alias)
            .await
            .map_err(control_identity_error)?;
        if let Some(target) = target {
            return crate::identity_first::IdentityRuntime::run_member_alias_targets_operation_tracked(
                vec![target],
                move || async move {
                    operation().await.map_err(|error| {
                        format!(
                            "{TRACKED_CONTROL_ERROR_PREFIX}{}",
                            serde_json::to_string(&error)
                                .unwrap_or_else(|_| "[\"identity_error\",\"unserializable control error\"]".to_string())
                        )
                    })
                },
            )
            .await
            .map_err(tracked_control_error);
        }
    }
    if crate::member_comms_id::is_reserved_generated_alias(&member_alias) {
        return Err((
            "identity_authority_unavailable".to_string(),
            format!("generated member alias '{member_alias}' requires the owning IdentityRuntime"),
        ));
    }
    operation().await
}

struct WireControlParams<'a> {
    remote_member: &'a str,
    local_peer_spec_address: &'a str,
    local_comms_name: &'a str,
    local_peer_id: &'a str,
    local_pubkey_b64: Option<&'a str>,
    wire: bool,
}

async fn handle_wire(
    handle: &meerkat_mob::MobHandle,
    params: WireControlParams<'_>,
    identity_runtime: Option<&std::sync::Arc<crate::identity_first::IdentityRuntime>>,
) -> ControlResponse {
    let pubkey = match params.local_pubkey_b64 {
        Some(s) if !s.is_empty() => match crate::auth::peer_keys::decode_pubkey_b64(s) {
            Ok(bytes) => Some(bytes),
            Err(err) => {
                return ControlResponse::Err {
                    code: "decode".to_string(),
                    message: format!("local_pubkey_b64: {err}"),
                };
            }
        },
        _ => None,
    };
    let spec_result = match pubkey {
        Some(bytes) => meerkat_core::comms::TrustedPeerDescriptor::unsigned_with_pubkey(
            params.local_comms_name,
            params.local_peer_id,
            bytes,
            params.local_peer_spec_address,
        ),
        None => meerkat_core::comms::TrustedPeerDescriptor::test_only_unsigned(
            params.local_comms_name,
            params.local_peer_id,
            params.local_peer_spec_address,
        ),
    };
    let spec = match spec_result {
        Ok(spec) => spec,
        Err(err) => {
            return ControlResponse::Err {
                code: "peer_spec".to_string(),
                message: err,
            };
        }
    };
    // The control plane speaks the public alias space; the local roster
    // holds comms-safe encoded ids (meerkat 0.7 MemberCommsName), so encode
    // at this boundary.
    let operation_handle = handle.clone();
    let operation_member =
        crate::member_comms_id::runtime_alias_str(params.remote_member).into_owned();
    let wire = params.wire;
    let result =
        run_control_member_operation(identity_runtime, params.remote_member, move || async move {
            let mid = crate::member_comms_id::mob_member_id(&operation_member);
            let result = if wire {
                operation_handle
                    .wire(mid, meerkat_mob::PeerTarget::External(spec))
                    .await
            } else {
                operation_handle
                    .unwire(mid, meerkat_mob::PeerTarget::External(spec))
                    .await
            };
            result.map_err(|error| ("mob_error".to_string(), error.to_string()))
        })
        .await;
    match result {
        Ok(()) => ControlResponse::Ok,
        Err((code, message)) => ControlResponse::Err { code, message },
    }
}

async fn handle_inject(
    handle: &meerkat_mob::MobHandle,
    remote_member: &str,
    content: serde_json::Value,
    identity_runtime: Option<&std::sync::Arc<crate::identity_first::IdentityRuntime>>,
) -> ControlResponse {
    let content_input: meerkat_core::ContentInput = match serde_json::from_value(content) {
        Ok(c) => c,
        Err(err) => {
            return ControlResponse::Err {
                code: "decode".to_string(),
                message: format!("content: {err}"),
            };
        }
    };
    let operation_handle = handle.clone();
    let operation_member = crate::member_comms_id::runtime_alias_str(remote_member).into_owned();
    match run_control_member_operation(identity_runtime, remote_member, move || async move {
        let mid = crate::member_comms_id::mob_member_id(&operation_member);
        operation_handle
            .member(&mid)
            .await
            .map_err(|error| ("unknown_member".to_string(), error.to_string()))?
            .send(content_input, meerkat_core::types::HandlingMode::Queue)
            .await
            .map_err(|error| ("mob_error".to_string(), error.to_string()))?;
        operation_handle
            .resolve_bridge_session_id(&mid)
            .await
            .map(|session_id| session_id.to_string())
            .ok_or_else(|| {
                (
                    "no_session".to_string(),
                    format!("member '{operation_member}' has no bound bridge session"),
                )
            })
    })
    .await
    {
        Ok(session_id) => ControlResponse::Injected { session_id },
        Err((code, message)) => ControlResponse::Err { code, message },
    }
}

async fn handle_lookup_member(
    handle: &meerkat_mob::MobHandle,
    remote_member: &str,
    identity_runtime: Option<&std::sync::Arc<crate::identity_first::IdentityRuntime>>,
) -> ControlResponse {
    let operation_handle = handle.clone();
    let operation_member = crate::member_comms_id::runtime_alias_str(remote_member).into_owned();
    match run_control_member_operation(identity_runtime, remote_member, move || async move {
        handle_lookup_member_raw(&operation_handle, &operation_member).await
    })
    .await
    {
        Ok(response) => response,
        Err((code, message)) => ControlResponse::Err { code, message },
    }
}

async fn handle_lookup_member_raw(
    handle: &meerkat_mob::MobHandle,
    remote_member: &str,
) -> Result<ControlResponse, (String, String)> {
    let mid = crate::member_comms_id::mob_member_id(remote_member);
    let mob_id = handle.mob_id().to_string();
    let entry = match handle.get_member(&mid).await {
        Ok(Some(e)) => e,
        Ok(None) => {
            return Err((
                "unknown_member".to_string(),
                format!("member '{remote_member}' not in mob '{mob_id}'"),
            ));
        }
        // A faulted lookup is a mob error, not an unknown member.
        Err(err) => {
            return Err(("mob_error".to_string(), err.to_string()));
        }
    };
    let peer_id = match entry.peer_id() {
        Some(p) => p.to_string(),
        None => {
            return Err((
                "no_comms".to_string(),
                format!("member '{remote_member}' has no comms runtime"),
            ));
        }
    };
    // The comms name derives from the roster id (the comms-safe encoding),
    // not the public alias. Build it through meerkat_core::MemberCommsName::new,
    // the fail-closed typed owner meerkat-mob routes all such names through, so
    // a slug-invalid component is rejected with a clear error rather than
    // minting a descriptor that silently fails to match at comms ingress.
    let comms_name = match meerkat_core::MemberCommsName::new(
        mob_id.as_str(),
        entry.role.as_str(),
        mid.as_str(),
    ) {
        Ok(name) => name.to_string(),
        Err(err) => {
            return Err((
                "invalid_comms_name".to_string(),
                format!(
                    "member '{remote_member}' in mob '{mob_id}' has an invalid comms name component: {err}"
                ),
            ));
        }
    };
    Ok(ControlResponse::Member {
        peer_id,
        comms_name,
    })
}

/// Run a control listener on `tcp_listener` until shutdown.
///
/// Each accepted connection is read-frame, dispatched to the handler,
/// response-written, then closed. The listener accepts indefinitely; cancel
/// via `tokio::select!` against your shutdown signal.
pub async fn serve_tcp_control(listener: TcpListener, handler: std::sync::Arc<dyn ControlHandler>) {
    loop {
        let (stream, _peer_addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(err) => {
                tracing::warn!(error = %err, "control listener accept failed; exiting");
                return;
            }
        };
        let handler = handler.clone();
        tokio::spawn(serve_one_tcp(stream, handler));
    }
}

/// Same as `serve_tcp_control` but for Unix-domain sockets.
#[cfg(unix)]
pub async fn serve_uds_control(
    listener: UnixListener,
    handler: std::sync::Arc<dyn ControlHandler>,
) {
    loop {
        let (stream, _peer_addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(err) => {
                tracing::warn!(error = %err, "uds control listener accept failed; exiting");
                return;
            }
        };
        let handler = handler.clone();
        tokio::spawn(serve_one_uds(stream, handler));
    }
}

async fn serve_one_tcp(stream: TcpStream, handler: std::sync::Arc<dyn ControlHandler>) {
    let mut s = ControlStream::Tcp(stream);
    serve_one(&mut s, handler).await;
}

#[cfg(unix)]
async fn serve_one_uds(stream: UnixStream, handler: std::sync::Arc<dyn ControlHandler>) {
    let mut s = ControlStream::Uds(stream);
    serve_one(&mut s, handler).await;
}

async fn serve_one(stream: &mut ControlStream, handler: std::sync::Arc<dyn ControlHandler>) {
    let payload = match stream.read_frame().await {
        Ok(buf) => buf,
        Err(err) => {
            tracing::debug!(error = %err, "control listener: read failed");
            return;
        }
    };
    let request = match serde_json::from_slice::<ControlRequest>(&payload) {
        Ok(req) => req,
        Err(err) => {
            let response = ControlResponse::Err {
                code: "decode".to_string(),
                message: err.to_string(),
            };
            let response_payload = serde_json::to_vec(&response).unwrap_or_default();
            let _ = stream.write_frame(&response_payload).await;
            return;
        }
    };
    let response = handler.handle(request).await;
    let response_payload = serde_json::to_vec(&response).unwrap_or_default();
    let _ = stream.write_frame(&response_payload).await;
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;
    use crate::identity_first::{
        AgentAddressability, AgentIdentity, AgentRuntimeId, CheckpointVersion,
        ContinuityGeneration, ContinuityRecord, DurabilityPolicy, DurableAgentSpec, FencingToken,
        IdentityLifecycleState, IdentityRuntime, IdentityRuntimeConfig, LeaseGrant,
        LocalContinuityStore, LocalLeaseProvider,
    };
    use std::collections::BTreeMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    struct EchoHandler;

    async fn identity_runtime_with_current_alias()
    -> Result<Arc<IdentityRuntime>, Box<dyn std::error::Error + Send + Sync>> {
        let runtime = Arc::new(IdentityRuntime::new(IdentityRuntimeConfig {
            continuity_store: Arc::new(LocalContinuityStore::in_memory()?),
            lease_provider: Arc::new(LocalLeaseProvider::new()),
            runtime_instance_id: "cross-mob-control-test".to_string(),
            has_runtime_store: true,
            durability_policy: DurabilityPolicy::SyncWriteThrough,
            bridge: None,
            default_timeout: None,
        }));
        let identity = AgentIdentity::parse("worker")?;
        runtime
            .register(
                DurableAgentSpec {
                    identity: identity.clone(),
                    profile: meerkat_mob::ProfileName::from("worker"),
                    addressability: AgentAddressability::Addressable,
                    display_name: None,
                    labels: BTreeMap::new(),
                    context: None,
                    additional_instructions: Vec::new(),
                    initial_message: None,
                    runtime_mode_override: None,
                    backend: None,
                    binding: None,
                },
                IdentityLifecycleState::Active,
                Some(ContinuityRecord {
                    identity: identity.clone(),
                    agent_runtime_id: AgentRuntimeId::parse("rt:worker:0")?,
                    session_id: meerkat_core::types::SessionId::new(),
                    generation: ContinuityGeneration::new(0),
                    checkpoint_version: CheckpointVersion::new(0),
                }),
                Some(LeaseGrant {
                    identity,
                    fencing_token: FencingToken::new(1),
                    ttl: Duration::from_mins(1),
                }),
            )
            .await;
        Ok(runtime)
    }

    impl ControlHandler for EchoHandler {
        fn handle(
            &self,
            request: ControlRequest,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ControlResponse> + Send + '_>>
        {
            Box::pin(async move {
                match request {
                    ControlRequest::Wire { .. } | ControlRequest::Unwire { .. } => {
                        ControlResponse::Ok
                    }
                    ControlRequest::Inject { remote_member, .. } => ControlResponse::Injected {
                        session_id: format!("session-for-{remote_member}"),
                    },
                    ControlRequest::LookupMember { remote_member } => ControlResponse::Member {
                        peer_id: format!("peer-id-for-{remote_member}"),
                        comms_name: format!("mob/role/{remote_member}"),
                    },
                }
            })
        }
    }

    #[tokio::test]
    async fn tcp_round_trip_inject() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handler: Arc<dyn ControlHandler> = Arc::new(EchoHandler);
        let server = tokio::spawn(serve_tcp_control(listener, handler));

        let endpoint = RemoteEndpoint::Tcp(addr.to_string());
        let request = ControlRequest::Inject {
            remote_member: "alice".to_string(),
            content: serde_json::json!({"text": "hello"}),
        };
        let response = RemoteControlClient::send(&endpoint, &request, DEFAULT_CONTROL_TIMEOUT)
            .await
            .expect("control rpc");
        assert_eq!(
            response,
            ControlResponse::Injected {
                session_id: "session-for-alice".to_string(),
            },
        );
        server.abort();
    }

    #[tokio::test]
    async fn tcp_round_trip_wire() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handler: Arc<dyn ControlHandler> = Arc::new(EchoHandler);
        let server = tokio::spawn(serve_tcp_control(listener, handler));

        let endpoint = RemoteEndpoint::Tcp(addr.to_string());
        let request = ControlRequest::Wire {
            remote_member: "bob".to_string(),
            local_peer_spec_address: "tcp://127.0.0.1:9001".to_string(),
            local_comms_name: "demo/role/alice".to_string(),
            local_peer_id: "00000000-0000-4000-8000-000000000001".to_string(),
            local_pubkey_b64: None,
        };
        let response = RemoteControlClient::send(&endpoint, &request, DEFAULT_CONTROL_TIMEOUT)
            .await
            .expect("control rpc");
        assert_eq!(response, ControlResponse::Ok);
        server.abort();
    }

    #[tokio::test]
    async fn malformed_request_returns_decode_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handler: Arc<dyn ControlHandler> = Arc::new(EchoHandler);
        let _server = tokio::spawn(serve_tcp_control(listener, handler));

        // Send raw garbage bytes as a "control request" — server should
        // respond with a `decode` error instead of dropping the connection
        // silently.
        let mut stream = TcpStream::connect(addr).await.expect("connect");
        stream
            .write_all(&u32::to_be_bytes(5))
            .await
            .expect("write header");
        stream.write_all(b"hello").await.expect("write payload");
        stream.flush().await.expect("flush");

        let mut header = [0u8; 4];
        stream.read_exact(&mut header).await.expect("read header");
        let len = u32::from_be_bytes(header) as usize;
        let mut buf = vec![0u8; len];
        stream.read_exact(&mut buf).await.expect("read payload");
        let response: ControlResponse = serde_json::from_slice(&buf).expect("decode response");
        match response {
            ControlResponse::Err { code, .. } => assert_eq!(code, "decode"),
            other => panic!("expected decode error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn identity_control_rejects_stale_and_encoded_aliases_before_mutation()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let runtime = identity_runtime_with_current_alias().await?;
        let calls = Arc::new(AtomicUsize::new(0));

        for stale_alias in [
            "rt:worker:1".to_string(),
            crate::member_comms_id::mob_member_id_str("rt:worker:1").into_owned(),
        ] {
            let operation_calls = Arc::clone(&calls);
            let error =
                run_control_member_operation(Some(&runtime), &stale_alias, move || async move {
                    operation_calls.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, (String, String)>(())
                })
                .await
                .expect_err("stale generation must fail before the lower plane");
            assert_eq!(error.0, "stale_runtime_alias");
        }
        assert_eq!(calls.load(Ordering::SeqCst), 0);

        let operation_calls = Arc::clone(&calls);
        run_control_member_operation(Some(&runtime), "rt:worker:0", move || async move {
            operation_calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, (String, String)>(())
        })
        .await
        .expect("current generation is admitted");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        Ok(())
    }

    #[tokio::test]
    async fn generated_control_alias_without_authority_fails_closed() {
        let calls = Arc::new(AtomicUsize::new(0));
        let operation_calls = Arc::clone(&calls);
        let encoded = crate::member_comms_id::mob_member_id_str("rt:worker:0").into_owned();
        let error = run_control_member_operation(None, &encoded, move || async move {
            operation_calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, (String, String)>(())
        })
        .await
        .expect_err("generated aliases require identity authority");

        assert_eq!(error.0, "identity_authority_unavailable");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }
}
