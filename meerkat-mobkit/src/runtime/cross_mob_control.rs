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

use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};

use super::cross_mob_remote::{RemoteEndpoint, RemoteMobError};

/// Address a gateway binds its cross-mob control listener on.
///
/// Accepts the same spelling the contact directory uses for remote peers
/// (`tcp://host:port`, `uds:///path`), parsed through the same
/// [`crate::contact_directory`] transport parser so the two surfaces never
/// drift. `inproc` is rejected: a control listener only makes sense across
/// processes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlListenAddr {
    /// TCP `host:port`. Port 0 binds an ephemeral port; the bound port is
    /// reported by [`BoundControlListener::advertised_address`].
    Tcp(String),
    /// Unix-domain-socket path.
    Uds(String),
}

impl ControlListenAddr {
    /// Parse `tcp://host:port` or `uds:///path`.
    pub fn parse(s: &str) -> Result<Self, String> {
        match crate::contact_directory::parse_transport(s) {
            Some(crate::contact_directory::MobTransport::Tcp(addr)) => Ok(Self::Tcp(addr)),
            Some(crate::contact_directory::MobTransport::Uds(path)) => Ok(Self::Uds(path)),
            Some(crate::contact_directory::MobTransport::Inproc) => Err(
                "control listener requires a cross-process address (tcp://host:port or \
                 uds:///path); 'inproc' has no listener"
                    .to_string(),
            ),
            None => Err(format!(
                "invalid control listen address '{s}': expected tcp://host:port or uds:///path"
            )),
        }
    }
}

impl std::fmt::Display for ControlListenAddr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tcp(addr) => write!(f, "tcp://{addr}"),
            Self::Uds(path) => write!(f, "uds://{path}"),
        }
    }
}

/// A control listener that has been bound but not yet served.
///
/// Binding is split from serving so callers learn the concrete local
/// address (the real port for `tcp://host:0`) before the accept loop
/// starts, and can surface it to tests and peer configuration.
pub enum BoundControlListener {
    Tcp {
        listener: TcpListener,
        advertised: String,
    },
    #[cfg(unix)]
    Uds {
        listener: UnixListener,
        advertised: String,
    },
}

impl BoundControlListener {
    /// Bind `addr`. For TCP the advertised address carries the kernel-
    /// assigned port when the caller bound port 0. For UDS the advertised
    /// address is `uds://{path}`.
    pub async fn bind(addr: &ControlListenAddr) -> Result<Self, std::io::Error> {
        match addr {
            ControlListenAddr::Tcp(spec) => {
                let listener = TcpListener::bind(spec).await?;
                let local = listener.local_addr()?;
                Ok(Self::Tcp {
                    listener,
                    advertised: format!("tcp://{local}"),
                })
            }
            #[cfg(unix)]
            ControlListenAddr::Uds(path) => {
                let listener = UnixListener::bind(std::path::Path::new(path))?;
                Ok(Self::Uds {
                    listener,
                    advertised: format!("uds://{path}"),
                })
            }
            #[cfg(not(unix))]
            ControlListenAddr::Uds(_) => Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "unix domain sockets are not supported on this platform",
            )),
        }
    }

    /// The dialable address of this listener (`tcp://ip:port` with the
    /// real bound port, or `uds:///path`).
    pub fn advertised_address(&self) -> &str {
        match self {
            Self::Tcp { advertised, .. } => advertised,
            #[cfg(unix)]
            Self::Uds { advertised, .. } => advertised,
        }
    }

    /// Serve control requests until the owning task is aborted. Responses
    /// are signed with the gateway key when `signer` holds one (re-read
    /// per request, so late-installed keys take effect).
    pub async fn serve(
        self,
        handler: std::sync::Arc<dyn ControlHandler>,
        signer: ControlSignerSlot,
    ) {
        match self {
            Self::Tcp { listener, .. } => serve_tcp_control(listener, handler, signer).await,
            #[cfg(unix)]
            Self::Uds { listener, .. } => serve_uds_control(listener, handler, signer).await,
        }
    }
}

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
        /// Ed25519 transport pubkey of the calling MEMBER. When present,
        /// the receiving gateway builds a signed `TrustedPeerDescriptor`
        /// so meerkat-comms can verify envelope signatures.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        local_pubkey_b64: Option<String>,
        /// Client-minted freshness nonce. It rides inside the request
        /// bytes, so the response signature (which covers the request
        /// digest) cannot be replayed for a later request. Old servers
        /// ignore it; old clients omit it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nonce: Option<String>,
    },
    /// Unwire a previously-wired peer.
    Unwire {
        remote_member: String,
        local_peer_spec_address: String,
        local_comms_name: String,
        local_peer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        local_pubkey_b64: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nonce: Option<String>,
    },
    /// Inject an external-turn message into a remote member's session.
    /// Used by `send_cross_mob` for app-level injection.
    Inject {
        remote_member: String,
        /// JSON-encoded `meerkat_core::ContentInput`.
        content: serde_json::Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nonce: Option<String>,
    },
    /// Look up a member's comms info on the remote side. Used during
    /// `wire_cross_mob` to discover the remote member's peer_id and
    /// derived comms_name without requiring caller-supplied bookkeeping.
    LookupMember {
        remote_member: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nonce: Option<String>,
    },
}

/// Cross-mob control response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum ControlResponse {
    /// Operation succeeded (no payload).
    Ok {
        /// Gateway signature over this response bound to the exact request
        /// bytes - see [`control_response_signing_payload`]. Absent when
        /// the serving gateway has no signing keys installed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sig_b64: Option<String>,
    },
    /// Inject succeeded - the far side admitted the dispatch and returns
    /// the bridge session id that accepted it, so the caller can correlate
    /// downstream events.
    ///
    /// This is NOT a durability receipt. It classifies as dispatch
    /// admission only: the receiving runtime accepted the turn into the
    /// named session, and whether that turn commits durably is the
    /// receiving runtime's business. Coarse transport ACKs (this response
    /// included) must never be inferred as durable=true. Callers that need
    /// durable admission over remote paths reconcile by idempotent
    /// re-submit with WorkRef dedup - the protocol deliberately has no
    /// resend verb.
    Injected {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sig_b64: Option<String>,
    },
    /// LookupMember succeeded - return the remote member's comms facts so
    /// the caller can build a signed `TrustedPeerDescriptor` pointing at
    /// it.
    Member {
        peer_id: String,
        comms_name: String,
        /// The member's transport pubkey as the roster carries it (base64,
        /// optional `ed25519:` prefix). `None` when the member has no
        /// comms runtime. Descriptors for this member MUST use this key:
        /// the peer-id/pubkey consistency check rejects any other (the
        /// gateway key included).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pubkey_b64: Option<String>,
        /// The member's dialable envelope-listener address as its live
        /// comms runtime advertises it (`tcp://host:port` with the real
        /// bound port, `uds:///path`, or `inproc://name` for members
        /// without a socket transport). `None` when the serving gateway's
        /// control handler has no session-service access.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        advertised_address: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sig_b64: Option<String>,
    },
    /// Operation failed. `code` is a stable short string for machine
    /// dispatch (`unknown_member`, `mob_error`, `decode`, ...); `message`
    /// is human-readable.
    Err { code: String, message: String },
}

/// Domain-separation context for control-response signatures. Versioned so
/// a future payload change cannot be confused with this one.
pub const CONTROL_SIG_CONTEXT: &str = "mobkit-cross-mob-control-v1";

/// Live signing authority for control responses: the gateway keypair,
/// late-bound because hosts install keys after the runtime (and possibly
/// after the listener) exists. `None` serves unsigned responses, which
/// clients with a pinned peer pubkey reject.
pub type ControlSignerSlot = std::sync::Arc<
    std::sync::RwLock<Option<std::sync::Arc<crate::auth::peer_keys::GatewayPeerKeys>>>,
>;

/// A signer slot that never signs (tests, deployments without keys).
pub fn unsigned_control_signer() -> ControlSignerSlot {
    std::sync::Arc::new(std::sync::RwLock::new(None))
}

fn control_request_digest_hex(request_bytes: &[u8]) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(request_bytes);
    let digest = hasher.finalize();
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

/// The deterministic byte string a control-response signature covers.
///
/// Built from the semantic response fields plus the SHA-256 of the exact
/// request bytes the server read off the wire - never from re-serialized
/// JSON, so there is no canonicalization to get wrong. Binding the request
/// digest means a signature is only valid for the one request (including
/// its client-minted nonce) that elicited it: a MITM can neither
/// substitute member facts in a `Member` response nor replay a previous
/// response for a fresh request. `Err` responses are not trusted material
/// and are never signed.
pub fn control_response_signing_payload(
    request_bytes: &[u8],
    response: &ControlResponse,
) -> Option<String> {
    let facts = match response {
        ControlResponse::Ok { .. } => "ok".to_string(),
        ControlResponse::Injected { session_id, .. } => format!("injected\n{session_id}"),
        ControlResponse::Member {
            peer_id,
            comms_name,
            pubkey_b64,
            advertised_address,
            ..
        } => format!(
            "member\n{peer_id}\n{comms_name}\n{}\n{}",
            pubkey_b64.as_deref().unwrap_or(""),
            advertised_address.as_deref().unwrap_or("")
        ),
        ControlResponse::Err { .. } => return None,
    };
    Some(format!(
        "{CONTROL_SIG_CONTEXT}\n{}\n{facts}",
        control_request_digest_hex(request_bytes)
    ))
}

/// Sign `response` in place with the gateway key. No-op for `Err`.
fn sign_control_response(
    signer: &crate::auth::peer_keys::GatewayPeerKeys,
    request_bytes: &[u8],
    response: &mut ControlResponse,
) {
    use ed25519_dalek::Signer;
    let Some(payload) = control_response_signing_payload(request_bytes, response) else {
        return;
    };
    let signature = signer.signing_key().sign(payload.as_bytes());
    let encoded = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());
    match response {
        ControlResponse::Ok { sig_b64 }
        | ControlResponse::Injected { sig_b64, .. }
        | ControlResponse::Member { sig_b64, .. } => *sig_b64 = Some(encoded),
        ControlResponse::Err { .. } => {}
    }
}

/// Verify a control response against the pinned gateway pubkey and the
/// exact request bytes that were sent. `Err` responses pass unverified:
/// they are failure reports, not trusted material, and rejecting them
/// would only change WHICH error the caller sees.
pub fn verify_control_response(
    pinned_pubkey: &[u8; 32],
    request_bytes: &[u8],
    response: &ControlResponse,
) -> Result<(), String> {
    let Some(payload) = control_response_signing_payload(request_bytes, response) else {
        return Ok(());
    };
    let sig_b64 = match response {
        ControlResponse::Ok { sig_b64 }
        | ControlResponse::Injected { sig_b64, .. }
        | ControlResponse::Member { sig_b64, .. } => sig_b64.as_deref(),
        ControlResponse::Err { .. } => None,
    };
    let Some(sig_b64) = sig_b64 else {
        return Err(
            "response is unsigned; the peer gateway has no signing keys installed or predates \
             signed control responses"
                .to_string(),
        );
    };
    let sig_bytes = base64::engine::general_purpose::STANDARD
        .decode(sig_b64)
        .map_err(|err| format!("signature is not valid base64: {err}"))?;
    let sig_bytes: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| "signature must be 64 bytes".to_string())?;
    let signature = ed25519_dalek::Signature::from_bytes(&sig_bytes);
    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(pinned_pubkey)
        .map_err(|err| format!("pinned pubkey is not a valid Ed25519 key: {err}"))?;
    verifying_key
        .verify_strict(payload.as_bytes(), &signature)
        .map_err(|_| "signature does not verify against the pinned gateway pubkey".to_string())
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
        let payload =
            serde_json::to_vec(request).map_err(|err| encode_error(endpoint, err.to_string()))?;
        Self::send_payload(endpoint, &payload, timeout).await
    }

    /// Send pre-serialized request bytes. Callers that verify signed
    /// responses use this form so the exact bytes on the wire (the input
    /// to the response's request digest) are in their hands.
    pub async fn send_payload(
        endpoint: &RemoteEndpoint,
        payload: &[u8],
        timeout: Duration,
    ) -> Result<ControlResponse, RemoteMobError> {
        tokio::time::timeout(timeout, Self::send_inner(endpoint, payload))
            .await
            .map_err(|_| RemoteMobError::ControlChannelUnavailable {
                mob_id: String::new(),
                endpoint: endpoint.comms_address(),
                operation: "timeout",
            })?
    }

    async fn send_inner(
        endpoint: &RemoteEndpoint,
        payload: &[u8],
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
        stream
            .write_frame(payload)
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
    identity_authority: ControlIdentityAuthority,
    /// Session-service access for member-fact resolution: `LookupMember`
    /// reaches through it to the member's live comms runtime for the
    /// advertised envelope-listener address (the roster does not persist
    /// transport addresses). `None` degrades LookupMember to roster facts
    /// only, which remote wire callers reject fail-closed.
    session_service: Option<std::sync::Arc<dyn meerkat_mob::MobSessionService>>,
}

/// How the handler resolves the durable identity authority for generated
/// member aliases. `Shared` re-reads a host-owned slot on every request:
/// gateways attach identity-first AFTER the base runtime (and thus after
/// the control listener) exists, so capturing `identity_runtime()` at
/// listener start would permanently capture `None`.
enum ControlIdentityAuthority {
    None,
    Fixed(std::sync::Arc<crate::identity_first::IdentityRuntime>),
    Shared(
        std::sync::Arc<
            std::sync::RwLock<Option<std::sync::Arc<crate::identity_first::IdentityRuntime>>>,
        >,
    ),
}

impl ControlIdentityAuthority {
    fn current(&self) -> Option<std::sync::Arc<crate::identity_first::IdentityRuntime>> {
        match self {
            Self::None => None,
            Self::Fixed(runtime) => Some(std::sync::Arc::clone(runtime)),
            Self::Shared(slot) => slot
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone(),
        }
    }
}

impl MobHandleControlHandler {
    /// Construct a classic member-plane handler. Reserved generated aliases
    /// fail closed because no durable identity authority was supplied.
    pub fn new(handle: meerkat_mob::MobHandle) -> Self {
        Self {
            handle,
            identity_authority: ControlIdentityAuthority::None,
            session_service: None,
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
            identity_authority: ControlIdentityAuthority::Fixed(identity_runtime),
            session_service: None,
        }
    }

    /// Construct a handler whose identity authority is re-read from a
    /// host-owned slot on every request, so identity-first attachment that
    /// happens after the listener starts is still honored.
    pub fn with_shared_identity_authority(
        handle: meerkat_mob::MobHandle,
        identity_slot: std::sync::Arc<
            std::sync::RwLock<Option<std::sync::Arc<crate::identity_first::IdentityRuntime>>>,
        >,
    ) -> Self {
        Self {
            handle,
            identity_authority: ControlIdentityAuthority::Shared(identity_slot),
            session_service: None,
        }
    }

    /// Attach the session service that owns member comms runtimes, so
    /// `LookupMember` can report each member's advertised envelope-listener
    /// address alongside its roster facts.
    #[must_use]
    pub fn with_session_service(
        mut self,
        session_service: std::sync::Arc<dyn meerkat_mob::MobSessionService>,
    ) -> Self {
        self.session_service = Some(session_service);
        self
    }
}

impl ControlHandler for MobHandleControlHandler {
    fn handle(
        &self,
        request: ControlRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ControlResponse> + Send + '_>> {
        let handle = self.handle.clone();
        let identity_runtime = self.identity_authority.current();
        let session_service = self.session_service.clone();
        Box::pin(async move {
            match request {
                ControlRequest::Wire {
                    remote_member,
                    local_peer_spec_address,
                    local_comms_name,
                    local_peer_id,
                    local_pubkey_b64,
                    nonce: _,
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
                    nonce: _,
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
                    nonce: _,
                } => {
                    handle_inject(&handle, &remote_member, content, identity_runtime.as_ref()).await
                }
                ControlRequest::LookupMember {
                    remote_member,
                    nonce: _,
                } => {
                    handle_lookup_member(
                        &handle,
                        session_service.as_ref(),
                        &remote_member,
                        identity_runtime.as_ref(),
                    )
                    .await
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

/// Build the `TrustedPeerDescriptor` a remote Wire/Unwire request installs
/// on this side.
///
/// Requests arriving over the remote control channel must carry a peer
/// address that is dialable from THIS process and the calling MEMBER's own
/// transport pubkey. `inproc://` fails closed: it used to slip through the
/// relaxed unsigned branch and install a descriptor that could never
/// deliver an envelope across processes. A missing pubkey also fails
/// closed: meerkat-comms keys its trust store by pubkey, so an unsigned
/// row would admit any sender at ingress. The gateway key is not accepted
/// either - `unsigned_with_pubkey` enforces peer_id == UUIDv5(pubkey), and
/// only the member's transport key satisfies that.
fn build_remote_wire_descriptor(
    params: &WireControlParams<'_>,
) -> Result<meerkat_core::comms::TrustedPeerDescriptor, (String, String)> {
    if params.local_peer_spec_address.starts_with("inproc://") {
        return Err((
            "inproc_address_rejected".to_string(),
            format!(
                "peer address '{}' is inproc:// and unreachable from another process; the \
                 calling gateway must advertise a tcp:// or uds:// member comms address",
                params.local_peer_spec_address
            ),
        ));
    }
    let Some(pubkey_b64) = params.local_pubkey_b64.filter(|value| !value.is_empty()) else {
        return Err((
            "missing_pubkey".to_string(),
            "remote wire requests must carry the calling member's Ed25519 transport pubkey; \
             an unsigned descriptor would admit any sender at comms ingress"
                .to_string(),
        ));
    };
    let pubkey = crate::auth::peer_keys::decode_pubkey_b64(pubkey_b64)
        .map_err(|err| ("decode".to_string(), format!("local_pubkey_b64: {err}")))?;
    meerkat_core::comms::TrustedPeerDescriptor::unsigned_with_pubkey(
        params.local_comms_name,
        params.local_peer_id,
        pubkey,
        params.local_peer_spec_address,
    )
    .map_err(|err| ("peer_spec".to_string(), err))
}

async fn handle_wire(
    handle: &meerkat_mob::MobHandle,
    params: WireControlParams<'_>,
    identity_runtime: Option<&std::sync::Arc<crate::identity_first::IdentityRuntime>>,
) -> ControlResponse {
    let spec = match build_remote_wire_descriptor(&params) {
        Ok(spec) => spec,
        Err((code, message)) => return ControlResponse::Err { code, message },
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
        Ok(()) => ControlResponse::Ok { sig_b64: None },
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
        Ok(session_id) => ControlResponse::Injected {
            session_id,
            sig_b64: None,
        },
        Err((code, message)) => ControlResponse::Err { code, message },
    }
}

async fn handle_lookup_member(
    handle: &meerkat_mob::MobHandle,
    session_service: Option<&std::sync::Arc<dyn meerkat_mob::MobSessionService>>,
    remote_member: &str,
    identity_runtime: Option<&std::sync::Arc<crate::identity_first::IdentityRuntime>>,
) -> ControlResponse {
    let operation_handle = handle.clone();
    let operation_service = session_service.cloned();
    let operation_member = crate::member_comms_id::runtime_alias_str(remote_member).into_owned();
    match run_control_member_operation(identity_runtime, remote_member, move || async move {
        handle_lookup_member_raw(
            &operation_handle,
            operation_service.as_ref(),
            &operation_member,
        )
        .await
    })
    .await
    {
        Ok(response) => response,
        Err((code, message)) => ControlResponse::Err { code, message },
    }
}

async fn handle_lookup_member_raw(
    handle: &meerkat_mob::MobHandle,
    session_service: Option<&std::sync::Arc<dyn meerkat_mob::MobSessionService>>,
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
    let pubkey_b64 = entry.transport_public_key().map(str::to_string);
    // The member's dialable envelope-listener address lives on its live
    // comms runtime, not in the roster; resolve it through the session
    // service when the control handler was given one.
    let mut advertised_address = None;
    if let Some(service) = session_service
        && let Some(session_id) = handle.resolve_bridge_session_id(&mid).await
        && let Some(comms) = service.comms_runtime(&session_id).await
    {
        advertised_address = comms.advertised_address();
    }
    Ok(ControlResponse::Member {
        peer_id,
        comms_name,
        pubkey_b64,
        advertised_address,
        sig_b64: None,
    })
}

/// Run a control listener on `tcp_listener` until shutdown.
///
/// Each accepted connection is read-frame, dispatched to the handler,
/// response-written, then closed. The listener accepts indefinitely; cancel
/// via `tokio::select!` against your shutdown signal.
pub async fn serve_tcp_control(
    listener: TcpListener,
    handler: std::sync::Arc<dyn ControlHandler>,
    signer: ControlSignerSlot,
) {
    loop {
        let (stream, _peer_addr) = match listener.accept().await {
            Ok(pair) => pair,
            Err(err) => {
                tracing::warn!(error = %err, "control listener accept failed; exiting");
                return;
            }
        };
        let handler = handler.clone();
        let signer = std::sync::Arc::clone(&signer);
        tokio::spawn(serve_one_tcp(stream, handler, signer));
    }
}

/// Same as `serve_tcp_control` but for Unix-domain sockets.
#[cfg(unix)]
pub async fn serve_uds_control(
    listener: UnixListener,
    handler: std::sync::Arc<dyn ControlHandler>,
    signer: ControlSignerSlot,
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
        let signer = std::sync::Arc::clone(&signer);
        tokio::spawn(serve_one_uds(stream, handler, signer));
    }
}

async fn serve_one_tcp(
    stream: TcpStream,
    handler: std::sync::Arc<dyn ControlHandler>,
    signer: ControlSignerSlot,
) {
    let mut s = ControlStream::Tcp(stream);
    serve_one(&mut s, handler, signer).await;
}

#[cfg(unix)]
async fn serve_one_uds(
    stream: UnixStream,
    handler: std::sync::Arc<dyn ControlHandler>,
    signer: ControlSignerSlot,
) {
    let mut s = ControlStream::Uds(stream);
    serve_one(&mut s, handler, signer).await;
}

async fn serve_one(
    stream: &mut ControlStream,
    handler: std::sync::Arc<dyn ControlHandler>,
    signer: ControlSignerSlot,
) {
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
    let mut response = handler.handle(request).await;
    // Bind the response to this gateway's pinned identity and to the exact
    // request bytes. Re-read per request: hosts install keys after the
    // listener exists.
    let keys = signer
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    if let Some(keys) = keys {
        sign_control_response(&keys, &payload, &mut response);
    }
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
                        ControlResponse::Ok { sig_b64: None }
                    }
                    ControlRequest::Inject { remote_member, .. } => ControlResponse::Injected {
                        session_id: format!("session-for-{remote_member}"),
                        sig_b64: None,
                    },
                    ControlRequest::LookupMember { remote_member, .. } => ControlResponse::Member {
                        peer_id: format!("peer-id-for-{remote_member}"),
                        comms_name: format!("mob/role/{remote_member}"),
                        pubkey_b64: None,
                        advertised_address: None,
                        sig_b64: None,
                    },
                }
            })
        }
    }

    /// Base64 of the 32-byte pubkey `[42u8; 32]` (same constant the
    /// contact-directory tests use).
    const TEST_PUBKEY_B64: &str = "KioqKioqKioqKioqKioqKioqKioqKioqKioqKioqKio=";

    fn wire_params<'a>(
        address: &'a str,
        peer_id: &'a str,
        pubkey_b64: Option<&'a str>,
    ) -> WireControlParams<'a> {
        WireControlParams {
            remote_member: "bob",
            local_peer_spec_address: address,
            local_comms_name: "mob-a/worker/alice",
            local_peer_id: peer_id,
            local_pubkey_b64: pubkey_b64,
            wire: true,
        }
    }

    /// Regression: the remote Wire handler used to route inproc:// through
    /// the relaxed unsigned branch, silently installing a descriptor that
    /// could never deliver across processes.
    #[test]
    fn remote_wire_descriptor_rejects_inproc_addresses() {
        let params = wire_params(
            "inproc://mob-a/worker/alice",
            "00000000-0000-4000-8000-000000000001",
            Some(TEST_PUBKEY_B64),
        );
        let (code, _) = build_remote_wire_descriptor(&params).expect_err("inproc must fail");
        assert_eq!(code, "inproc_address_rejected");
    }

    #[test]
    fn remote_wire_descriptor_requires_pubkey() {
        for pubkey in [None, Some("")] {
            let params = wire_params(
                "tcp://127.0.0.1:9001",
                "00000000-0000-4000-8000-000000000001",
                pubkey,
            );
            let (code, _) =
                build_remote_wire_descriptor(&params).expect_err("missing pubkey must fail");
            assert_eq!(code, "missing_pubkey");
        }
    }

    #[test]
    fn remote_wire_descriptor_builds_signed_descriptor() {
        let derived_peer_id =
            meerkat_core::comms::PeerId::from_ed25519_pubkey(&[42u8; 32]).to_string();
        let params = wire_params(
            "tcp://127.0.0.1:9001",
            &derived_peer_id,
            Some(TEST_PUBKEY_B64),
        );
        let spec = build_remote_wire_descriptor(&params).expect("descriptor");
        assert_eq!(spec.pubkey, [42u8; 32]);
        assert_eq!(spec.address.endpoint(), "127.0.0.1:9001");
    }

    /// The peer-id/pubkey consistency check is what forces the MEMBER
    /// transport key onto this path: a mismatched key (e.g. the gateway
    /// key) must be rejected, not installed.
    #[test]
    fn remote_wire_descriptor_rejects_mismatched_pubkey() {
        let params = wire_params(
            "tcp://127.0.0.1:9001",
            "00000000-0000-4000-8000-000000000001",
            Some(TEST_PUBKEY_B64),
        );
        let (code, _) =
            build_remote_wire_descriptor(&params).expect_err("mismatched pubkey must fail");
        assert_eq!(code, "peer_spec");
    }

    fn signer_with(keys: crate::auth::peer_keys::GatewayPeerKeys) -> ControlSignerSlot {
        Arc::new(std::sync::RwLock::new(Some(Arc::new(keys))))
    }

    /// A signed response verifies against the pinned key and the exact
    /// request bytes - and against nothing else: different request bytes
    /// (a replay for a fresh nonce) and a different pinned key both fail.
    #[tokio::test]
    async fn signed_responses_verify_against_pinned_gateway_key() {
        let keys = crate::auth::peer_keys::GatewayPeerKeys::ephemeral();
        let pinned = keys.pubkey_bytes();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handler: Arc<dyn ControlHandler> = Arc::new(EchoHandler);
        let server = tokio::spawn(serve_tcp_control(listener, handler, signer_with(keys)));

        let request = ControlRequest::LookupMember {
            remote_member: "alice".to_string(),
            nonce: Some("nonce-1".to_string()),
        };
        let payload = serde_json::to_vec(&request).expect("encode");
        let endpoint = RemoteEndpoint::Tcp(addr.to_string());
        let response =
            RemoteControlClient::send_payload(&endpoint, &payload, DEFAULT_CONTROL_TIMEOUT)
                .await
                .expect("control rpc");
        verify_control_response(&pinned, &payload, &response).expect("signature verifies");

        let replayed_request = ControlRequest::LookupMember {
            remote_member: "alice".to_string(),
            nonce: Some("nonce-2".to_string()),
        };
        let replayed_payload = serde_json::to_vec(&replayed_request).expect("encode");
        assert!(
            verify_control_response(&pinned, &replayed_payload, &response).is_err(),
            "a response must not verify for different request bytes (replay)"
        );
        let other_key = crate::auth::peer_keys::GatewayPeerKeys::ephemeral().pubkey_bytes();
        assert!(
            verify_control_response(&other_key, &payload, &response).is_err(),
            "a response must not verify against a different pinned key"
        );
        server.abort();
    }

    #[test]
    fn unsigned_response_fails_verification_when_pinned() {
        let pinned = crate::auth::peer_keys::GatewayPeerKeys::ephemeral().pubkey_bytes();
        let response = ControlResponse::Ok { sig_b64: None };
        let err =
            verify_control_response(&pinned, b"{}", &response).expect_err("unsigned must fail");
        assert!(err.contains("unsigned"), "got: {err}");
    }

    /// `Err` responses are failure reports, not trusted material: they
    /// carry no signature and pass verification unchanged (rejecting them
    /// would only swap WHICH error the caller sees).
    #[test]
    fn error_responses_skip_verification() {
        let pinned = crate::auth::peer_keys::GatewayPeerKeys::ephemeral().pubkey_bytes();
        let response = ControlResponse::Err {
            code: "unknown_member".to_string(),
            message: "nope".to_string(),
        };
        verify_control_response(&pinned, b"{}", &response).expect("Err passes through");
    }

    /// Strict-when-pinned at the proxy: a contact that pins a gateway
    /// pubkey rejects unsigned responses typed; the explicit
    /// `require_signed_control = false` opt-out accepts them.
    #[tokio::test]
    async fn pinned_contact_requires_signed_control_responses() {
        use crate::contact_directory::{ContactEntry, MobTransport};
        use crate::runtime::cross_mob_remote::RemoteMobProxy;

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handler: Arc<dyn ControlHandler> = Arc::new(EchoHandler);
        let server = tokio::spawn(serve_tcp_control(
            listener,
            handler,
            unsigned_control_signer(),
        ));
        let pinned = crate::auth::peer_keys::GatewayPeerKeys::ephemeral().pubkey_bytes();

        let strict_entry = ContactEntry {
            mob_id: "remote".to_string(),
            transport: MobTransport::Tcp(addr.to_string()),
            pubkey: Some(pinned),
            require_signed_control: None,
        };
        let proxy = RemoteMobProxy::from_entry(&strict_entry)
            .expect("tcp ok")
            .expect("some");
        let err = proxy
            .lookup_member("alice")
            .await
            .expect_err("unsigned response must fail a pinned contact");
        assert!(
            matches!(err, RemoteMobError::ControlResponseUnauthenticated { .. }),
            "got {err:?}"
        );

        let opt_out_entry = ContactEntry {
            require_signed_control: Some(false),
            ..strict_entry
        };
        let proxy = RemoteMobProxy::from_entry(&opt_out_entry)
            .expect("tcp ok")
            .expect("some");
        proxy
            .lookup_member("alice")
            .await
            .expect("explicit opt-out accepts unsigned responses");
        server.abort();
    }

    /// End-to-end signed exchange through the proxy: a pinned contact
    /// talking to a gateway that signs with the matching key succeeds.
    #[tokio::test]
    async fn pinned_contact_accepts_signed_responses() {
        use crate::contact_directory::{ContactEntry, MobTransport};
        use crate::runtime::cross_mob_remote::RemoteMobProxy;

        let keys = crate::auth::peer_keys::GatewayPeerKeys::ephemeral();
        let pinned = keys.pubkey_bytes();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handler: Arc<dyn ControlHandler> = Arc::new(EchoHandler);
        let server = tokio::spawn(serve_tcp_control(listener, handler, signer_with(keys)));

        let entry = ContactEntry {
            mob_id: "remote".to_string(),
            transport: MobTransport::Tcp(addr.to_string()),
            pubkey: Some(pinned),
            require_signed_control: None,
        };
        let proxy = RemoteMobProxy::from_entry(&entry)
            .expect("tcp ok")
            .expect("some");
        let info = proxy
            .lookup_member("alice")
            .await
            .expect("signed lookup verifies");
        assert_eq!(info.peer_id, "peer-id-for-alice");
        proxy
            .wire_remote(
                "alice",
                "tcp://127.0.0.1:9001",
                "demo/role/alice",
                "00000000-0000-4000-8000-000000000001",
                None,
            )
            .await
            .expect("signed wire ack verifies");
        server.abort();
    }

    #[test]
    fn control_listen_addr_parses_contact_directory_spellings() {
        assert_eq!(
            ControlListenAddr::parse("tcp://127.0.0.1:9001"),
            Ok(ControlListenAddr::Tcp("127.0.0.1:9001".to_string())),
        );
        assert_eq!(
            ControlListenAddr::parse("uds:///var/run/mob.sock"),
            Ok(ControlListenAddr::Uds("/var/run/mob.sock".to_string())),
        );
        assert!(ControlListenAddr::parse("inproc").is_err());
        assert!(ControlListenAddr::parse("ftp://nope").is_err());
        assert!(ControlListenAddr::parse("127.0.0.1:9001").is_err());
    }

    #[test]
    fn control_listen_addr_display_round_trips() {
        for spec in ["tcp://127.0.0.1:9001", "uds:///var/run/mob.sock"] {
            let addr = ControlListenAddr::parse(spec).expect("parse");
            assert_eq!(addr.to_string(), spec);
        }
    }

    /// `tcp://host:0` binds an ephemeral port and the bound listener reports
    /// the real dialable address; a control request round-trips through it.
    #[tokio::test]
    async fn bound_listener_reports_real_port_and_serves() {
        let addr = ControlListenAddr::parse("tcp://127.0.0.1:0").expect("parse");
        let bound = BoundControlListener::bind(&addr).await.expect("bind");
        let advertised = bound.advertised_address().to_string();
        let dial = advertised.strip_prefix("tcp://").expect("tcp scheme");
        assert!(
            !dial.ends_with(":0"),
            "advertised address must carry the kernel-assigned port: {advertised}"
        );
        let handler: Arc<dyn ControlHandler> = Arc::new(EchoHandler);
        let server = tokio::spawn(bound.serve(handler, unsigned_control_signer()));

        let endpoint = RemoteEndpoint::Tcp(dial.to_string());
        let request = ControlRequest::LookupMember {
            remote_member: "alice".to_string(),
            nonce: None,
        };
        let response = RemoteControlClient::send(&endpoint, &request, DEFAULT_CONTROL_TIMEOUT)
            .await
            .expect("control rpc");
        assert!(matches!(response, ControlResponse::Member { .. }));
        server.abort();
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn bound_uds_listener_serves() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("control.sock");
        let addr = ControlListenAddr::Uds(path.display().to_string());
        let bound = BoundControlListener::bind(&addr).await.expect("bind");
        assert_eq!(
            bound.advertised_address(),
            format!("uds://{}", path.display())
        );
        let handler: Arc<dyn ControlHandler> = Arc::new(EchoHandler);
        let server = tokio::spawn(bound.serve(handler, unsigned_control_signer()));

        let endpoint = RemoteEndpoint::Uds(path.display().to_string());
        let request = ControlRequest::Inject {
            remote_member: "bob".to_string(),
            content: serde_json::json!({"text": "hi"}),
            nonce: None,
        };
        let response = RemoteControlClient::send(&endpoint, &request, DEFAULT_CONTROL_TIMEOUT)
            .await
            .expect("control rpc");
        assert_eq!(
            response,
            ControlResponse::Injected {
                session_id: "session-for-bob".to_string(),
                sig_b64: None,
            },
        );
        server.abort();
    }

    #[tokio::test]
    async fn tcp_round_trip_inject() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handler: Arc<dyn ControlHandler> = Arc::new(EchoHandler);
        let server = tokio::spawn(serve_tcp_control(
            listener,
            handler,
            unsigned_control_signer(),
        ));

        let endpoint = RemoteEndpoint::Tcp(addr.to_string());
        let request = ControlRequest::Inject {
            remote_member: "alice".to_string(),
            content: serde_json::json!({"text": "hello"}),
            nonce: None,
        };
        let response = RemoteControlClient::send(&endpoint, &request, DEFAULT_CONTROL_TIMEOUT)
            .await
            .expect("control rpc");
        assert_eq!(
            response,
            ControlResponse::Injected {
                session_id: "session-for-alice".to_string(),
                sig_b64: None,
            },
        );
        server.abort();
    }

    #[tokio::test]
    async fn tcp_round_trip_wire() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handler: Arc<dyn ControlHandler> = Arc::new(EchoHandler);
        let server = tokio::spawn(serve_tcp_control(
            listener,
            handler,
            unsigned_control_signer(),
        ));

        let endpoint = RemoteEndpoint::Tcp(addr.to_string());
        let request = ControlRequest::Wire {
            remote_member: "bob".to_string(),
            local_peer_spec_address: "tcp://127.0.0.1:9001".to_string(),
            local_comms_name: "demo/role/alice".to_string(),
            local_peer_id: "00000000-0000-4000-8000-000000000001".to_string(),
            local_pubkey_b64: None,
            nonce: None,
        };
        let response = RemoteControlClient::send(&endpoint, &request, DEFAULT_CONTROL_TIMEOUT)
            .await
            .expect("control rpc");
        assert_eq!(response, ControlResponse::Ok { sig_b64: None });
        server.abort();
    }

    #[tokio::test]
    async fn malformed_request_returns_decode_error() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        let handler: Arc<dyn ControlHandler> = Arc::new(EchoHandler);
        let _server = tokio::spawn(serve_tcp_control(
            listener,
            handler,
            unsigned_control_signer(),
        ));

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
