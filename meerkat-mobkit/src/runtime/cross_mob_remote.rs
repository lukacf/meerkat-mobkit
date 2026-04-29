//! Cross-mob remote-handle proxy.
//!
//! `cross_mob.rs` historically dispatched all wire/unwire/send operations
//! against an `Arc<MobHandle>` registered via `register_peer_mob`, which
//! only works for peers living in the *same* process (inproc transport).
//! This module introduces the seam for **remote** peers — mobs that live
//! in a different process or on a different host, reachable over TCP or
//! UDS.
//!
//! # Design (Phase 1)
//!
//! Per the 0.6 cross-mob plan we picked design **(A)** — a `LocalOrRemote`
//! enum at the cross-mob dispatch site (`cross_mob.rs`). The local arm
//! uses the existing `MobHandle` path; the remote arm uses [`RemoteMobProxy`]
//! to talk to the peer mob's admin endpoint over TCP/UDS.
//!
//! Phase 1 lands the **structural seam**: the `RemoteMobProxy` type, the
//! `LocalOrRemote` enum, and the contact-directory scheme (`tcp://host:port`,
//! `uds:///path`) flowing all the way through. Wire setup at the comms
//! layer already supports those addresses — outbound message routing uses
//! `meerkat_comms::Router`, which dispatches by `PeerAddr` scheme. The
//! per-member transport just works once peer specs carry the correct
//! address.
//!
//! What this module does **not** ship in Phase 1:
//!
//! 1. **Cross-process control RPC.** A real `RemoteMobProxy::wire` would
//!    open a TCP/UDS connection to the peer mob's admin endpoint, send a
//!    `WIRE` request, and await acknowledgment. Today the methods on
//!    `RemoteMobProxy` return [`RemoteMobError::ControlChannelUnavailable`]
//!    so callers learn the seam exists but is not yet wired.
//! 2. **Ed25519-signed peer descriptors.** Sibling Unit 4 lands signed
//!    descriptor authoring; this unit keeps using
//!    `TrustedPeerSpec::new(...)` (which is structurally `test_only_unsigned`
//!    in the 0.5.x core seam). The seam where Unit 4 will plug in is the
//!    helpers `build_tcp_peer_spec` / `build_uds_peer_spec` in
//!    `cross_mob.rs`.
//!
//! # Phase 2 plan (out of scope here)
//!
//! - Add a small JSON-over-CBOR control protocol with three operations:
//!   `WireRequest { local_member, peer_spec }`,
//!   `UnwireRequest { local_member, peer_spec }`,
//!   `InjectRequest { remote_member, content, handling_mode }`.
//!   Frame with `meerkat_comms::transport::codec::TransportCodec` (length-prefix
//!   CBOR) — same wire shape as agent envelopes, distinct payload type.
//! - Bind a control listener on each `UnifiedRuntime` startup when the
//!   contact directory advertises a TCP/UDS endpoint for *this* mob.
//! - `RemoteMobProxy` opens the connection lazily and reuses it for
//!   subsequent calls; reconnect on drop.

use crate::contact_directory::{ContactEntry, MobTransport};

/// Errors raised by the remote-mob proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteMobError {
    /// The contact directory entry uses an unsupported transport.
    UnsupportedTransport { mob_id: String, transport: String },
    /// The cross-process control channel for this peer mob is not
    /// reachable: connect failed, write/read failed, or the peer timed
    /// out. The `operation` field reports which RPC stage hit the wall.
    ControlChannelUnavailable {
        mob_id: String,
        endpoint: String,
        operation: &'static str,
    },
    /// The peer rejected our request with an error response. Carries the
    /// peer's stable `code` (e.g. `unknown_member`, `mob_error`) and a
    /// human-readable message.
    Rejected {
        mob_id: String,
        endpoint: String,
        code: String,
        message: String,
    },
    /// We failed to encode the request payload — usually a programmer
    /// error (un-serializable JSON value passed in).
    Encode { endpoint: String, message: String },
    /// We received a response we couldn't decode — peer is speaking a
    /// different protocol or sent corrupted data.
    Decode { endpoint: String, message: String },
}

impl RemoteMobError {
    /// Attach IO-error context to a `ControlChannelUnavailable` variant.
    /// Internal helper for the control-protocol module — keeps the public
    /// shape stable while letting us record the underlying message.
    pub(crate) fn with_context(self, context: String) -> Self {
        match self {
            Self::ControlChannelUnavailable {
                mob_id,
                endpoint,
                operation,
            } => Self::ControlChannelUnavailable {
                mob_id: if mob_id.is_empty() { context } else { mob_id },
                endpoint,
                operation,
            },
            other => other,
        }
    }
}

impl std::fmt::Display for RemoteMobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedTransport { mob_id, transport } => {
                write!(
                    f,
                    "remote mob '{mob_id}' uses unsupported transport: {transport}"
                )
            }
            Self::ControlChannelUnavailable {
                mob_id,
                endpoint,
                operation,
            } => {
                write!(
                    f,
                    "remote mob '{mob_id}' control channel ({endpoint}): operation '{operation}' \
                     failed — confirm the peer gateway is running with a control listener bound \
                     on this endpoint"
                )
            }
            Self::Rejected {
                mob_id,
                endpoint,
                code,
                message,
            } => {
                write!(
                    f,
                    "remote mob '{mob_id}' control channel ({endpoint}) rejected request \
                     [{code}]: {message}"
                )
            }
            Self::Encode { endpoint, message } => {
                write!(f, "control request encode failed for {endpoint}: {message}")
            }
            Self::Decode { endpoint, message } => {
                write!(
                    f,
                    "control response decode failed for {endpoint}: {message}"
                )
            }
        }
    }
}

impl std::error::Error for RemoteMobError {}

/// Endpoint address for the peer mob's control channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteEndpoint {
    /// TCP `host:port`.
    Tcp(String),
    /// Unix-domain-socket path.
    Uds(String),
}

impl RemoteEndpoint {
    /// Return the comms-style transport scheme (`tcp` or `uds`).
    pub fn scheme(&self) -> &'static str {
        match self {
            Self::Tcp(_) => "tcp",
            Self::Uds(_) => "uds",
        }
    }

    /// Return a comms-style address for this endpoint
    /// (`tcp://host:port` or `uds:///path`).
    pub fn comms_address(&self) -> String {
        match self {
            Self::Tcp(addr) => format!("tcp://{addr}"),
            Self::Uds(path) => format!("uds://{path}"),
        }
    }

    /// Bare endpoint string (without scheme) — `host:port` or `/path`.
    pub fn raw(&self) -> &str {
        match self {
            Self::Tcp(s) | Self::Uds(s) => s.as_str(),
        }
    }
}

/// A proxy for a mob that lives in a different process.
///
/// Phase 1: a structural placeholder. The methods are stubs that return
/// [`RemoteMobError::ControlChannelUnavailable`] so call sites can be
/// written against the final shape; Phase 2 will replace these with a
/// real control-channel client.
#[derive(Debug, Clone)]
pub struct RemoteMobProxy {
    mob_id: String,
    endpoint: RemoteEndpoint,
}

impl RemoteMobProxy {
    /// Build a proxy from a contact-directory entry. Returns `None` for
    /// `Inproc` entries — those are served by an `Arc<MobHandle>` via
    /// `LocalOrRemote::Local` instead.
    pub fn from_entry(entry: &ContactEntry) -> Result<Option<Self>, RemoteMobError> {
        match &entry.transport {
            MobTransport::Inproc => Ok(None),
            MobTransport::Tcp(addr) => Ok(Some(Self {
                mob_id: entry.mob_id.clone(),
                endpoint: RemoteEndpoint::Tcp(addr.clone()),
            })),
            MobTransport::Uds(path) => Ok(Some(Self {
                mob_id: entry.mob_id.clone(),
                endpoint: RemoteEndpoint::Uds(path.clone()),
            })),
        }
    }

    /// The peer mob's identifier.
    pub fn mob_id(&self) -> &str {
        &self.mob_id
    }

    /// The peer mob's control endpoint.
    pub fn endpoint(&self) -> &RemoteEndpoint {
        &self.endpoint
    }

    /// Wire a peer on the remote side.
    ///
    /// Sends a `Wire` control request to the peer gateway over its
    /// configured TCP/UDS endpoint. The remote side calls
    /// `MobHandle::wire(local_member, PeerTarget::External(spec))` and
    /// responds with `Ok` or `Err`.
    pub async fn wire_remote(
        &self,
        remote_member: &str,
        local_peer_spec_address: &str,
        local_comms_name: &str,
        local_peer_id: &str,
        local_pubkey_b64: Option<String>,
    ) -> Result<(), RemoteMobError> {
        let request = super::cross_mob_control::ControlRequest::Wire {
            remote_member: remote_member.to_string(),
            local_peer_spec_address: local_peer_spec_address.to_string(),
            local_comms_name: local_comms_name.to_string(),
            local_peer_id: local_peer_id.to_string(),
            local_pubkey_b64,
        };
        self.dispatch_no_payload(request, "wire").await
    }

    /// Unwire a peer on the remote side. Symmetric with [`Self::wire_remote`].
    pub async fn unwire_remote(
        &self,
        remote_member: &str,
        local_peer_spec_address: &str,
        local_comms_name: &str,
        local_peer_id: &str,
        local_pubkey_b64: Option<String>,
    ) -> Result<(), RemoteMobError> {
        let request = super::cross_mob_control::ControlRequest::Unwire {
            remote_member: remote_member.to_string(),
            local_peer_spec_address: local_peer_spec_address.to_string(),
            local_comms_name: local_comms_name.to_string(),
            local_peer_id: local_peer_id.to_string(),
            local_pubkey_b64,
        };
        self.dispatch_no_payload(request, "unwire").await
    }

    /// Inject an external-turn message into a remote member's session.
    ///
    /// Returns the bridge session id that accepted the injection, so the
    /// caller can correlate downstream events. Mirrors the local
    /// `MobHandle::send` shape used by `send_cross_mob`.
    pub async fn inject_message(
        &self,
        remote_member: &str,
        content_json: serde_json::Value,
    ) -> Result<String, RemoteMobError> {
        let request = super::cross_mob_control::ControlRequest::Inject {
            remote_member: remote_member.to_string(),
            content: content_json,
        };
        let response = super::cross_mob_control::RemoteControlClient::send(
            &self.endpoint,
            &request,
            super::cross_mob_control::DEFAULT_CONTROL_TIMEOUT,
        )
        .await
        .map_err(|err| self.attach_mob_id(err))?;
        match response {
            super::cross_mob_control::ControlResponse::Injected { session_id } => Ok(session_id),
            super::cross_mob_control::ControlResponse::Err { code, message } => {
                Err(RemoteMobError::Rejected {
                    mob_id: self.mob_id.clone(),
                    endpoint: self.endpoint.comms_address(),
                    code,
                    message,
                })
            }
            other => Err(RemoteMobError::Decode {
                endpoint: self.endpoint.comms_address(),
                message: format!("expected Injected response, got {other:?}"),
            }),
        }
    }

    async fn dispatch_no_payload(
        &self,
        request: super::cross_mob_control::ControlRequest,
        operation: &'static str,
    ) -> Result<(), RemoteMobError> {
        let response = super::cross_mob_control::RemoteControlClient::send(
            &self.endpoint,
            &request,
            super::cross_mob_control::DEFAULT_CONTROL_TIMEOUT,
        )
        .await
        .map_err(|err| self.attach_mob_id(err))?;
        match response {
            super::cross_mob_control::ControlResponse::Ok => Ok(()),
            super::cross_mob_control::ControlResponse::Err { code, message } => {
                Err(RemoteMobError::Rejected {
                    mob_id: self.mob_id.clone(),
                    endpoint: self.endpoint.comms_address(),
                    code,
                    message,
                })
            }
            other => Err(RemoteMobError::Decode {
                endpoint: self.endpoint.comms_address(),
                message: format!("expected Ok for {operation}, got {other:?}"),
            }),
        }
    }

    /// Look up a remote member's peer info via the control channel.
    /// Used during `wire_cross_mob` to discover what `TrustedPeerDescriptor`
    /// to build for the local-side wire without caller-supplied bookkeeping.
    pub async fn lookup_member(
        &self,
        remote_member: &str,
    ) -> Result<(String, String), RemoteMobError> {
        let request = super::cross_mob_control::ControlRequest::LookupMember {
            remote_member: remote_member.to_string(),
        };
        let response = super::cross_mob_control::RemoteControlClient::send(
            &self.endpoint,
            &request,
            super::cross_mob_control::DEFAULT_CONTROL_TIMEOUT,
        )
        .await
        .map_err(|err| self.attach_mob_id(err))?;
        match response {
            super::cross_mob_control::ControlResponse::Member {
                peer_id,
                comms_name,
            } => Ok((peer_id, comms_name)),
            super::cross_mob_control::ControlResponse::Err { code, message } => {
                Err(RemoteMobError::Rejected {
                    mob_id: self.mob_id.clone(),
                    endpoint: self.endpoint.comms_address(),
                    code,
                    message,
                })
            }
            other => Err(RemoteMobError::Decode {
                endpoint: self.endpoint.comms_address(),
                message: format!("expected Member response, got {other:?}"),
            }),
        }
    }

    fn attach_mob_id(&self, err: RemoteMobError) -> RemoteMobError {
        match err {
            RemoteMobError::ControlChannelUnavailable {
                mob_id,
                endpoint,
                operation,
            } if mob_id.is_empty() || mob_id != self.mob_id => {
                RemoteMobError::ControlChannelUnavailable {
                    mob_id: self.mob_id.clone(),
                    endpoint,
                    operation,
                }
            }
            other => other,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn from_entry_inproc_returns_none() {
        let entry = ContactEntry {
            mob_id: "demo".to_string(),
            transport: MobTransport::Inproc,
            pubkey: None,
        };
        let proxy = RemoteMobProxy::from_entry(&entry).expect("inproc is supported");
        assert!(proxy.is_none());
    }

    #[test]
    fn from_entry_tcp_round_trip() {
        let entry = ContactEntry {
            mob_id: "remote".to_string(),
            transport: MobTransport::Tcp("127.0.0.1:9001".to_string()),
            pubkey: None,
        };
        let proxy = RemoteMobProxy::from_entry(&entry)
            .expect("tcp is supported")
            .expect("tcp returns Some(proxy)");
        assert_eq!(proxy.mob_id(), "remote");
        assert_eq!(proxy.endpoint().scheme(), "tcp");
        assert_eq!(proxy.endpoint().raw(), "127.0.0.1:9001");
        assert_eq!(proxy.endpoint().comms_address(), "tcp://127.0.0.1:9001");
    }

    #[test]
    fn from_entry_uds_round_trip() {
        let entry = ContactEntry {
            mob_id: "remote-uds".to_string(),
            transport: MobTransport::Uds("/tmp/cross-mob.sock".to_string()),
            pubkey: None,
        };
        let proxy = RemoteMobProxy::from_entry(&entry)
            .expect("uds is supported")
            .expect("uds returns Some(proxy)");
        assert_eq!(proxy.endpoint().scheme(), "uds");
        assert_eq!(proxy.endpoint().raw(), "/tmp/cross-mob.sock");
        assert_eq!(
            proxy.endpoint().comms_address(),
            "uds:///tmp/cross-mob.sock"
        );
    }

    /// When no listener is bound at the configured endpoint, `wire_remote`
    /// surfaces `ControlChannelUnavailable` with the mob id attached.
    /// (End-to-end happy path lives in `tests/cross_mob_tcp.rs` once
    /// the unified runtime spins up its own control listener.)
    #[tokio::test]
    async fn wire_remote_returns_unavailable_when_no_listener() {
        let entry = ContactEntry {
            mob_id: "remote".to_string(),
            transport: MobTransport::Tcp("127.0.0.1:1".to_string()),
            pubkey: None,
        };
        let proxy = RemoteMobProxy::from_entry(&entry)
            .expect("tcp ok")
            .expect("some");
        let err = proxy
            .wire_remote(
                "alice",
                "tcp://127.0.0.1:9000",
                "demo/role/alice",
                "00000000-0000-4000-8000-000000000001",
                None,
            )
            .await
            .expect_err("no listener");
        assert!(
            matches!(err, RemoteMobError::ControlChannelUnavailable { ref mob_id, .. } if mob_id == "remote"),
            "got {err:?}"
        );
    }
}
