//! Cross-mob communication — peering and messaging between members in different mobs.

use meerkat_core::comms::TrustedPeerDescriptor;
use meerkat_core::types::HandlingMode;
use meerkat_mob::ids::MeerkatId;
use meerkat_mob::{MobHandle, PeerTarget};

use crate::auth::peer_keys::GatewayPeerKeys;
use crate::contact_directory::{ContactDirectory, ContactEntry, MobTransport};
use crate::runtime::cross_mob_remote::{RemoteMobError, RemoteMobProxy};

use super::UnifiedRuntime;

/// Dispatch a cross-mob operation to either an in-process `MobHandle`
/// (registered via `register_peer_mob`) or a [`RemoteMobProxy`] for
/// peers reachable over TCP/UDS.
///
/// Phase 1 wires the structural seam — see `runtime/cross_mob_remote.rs`
/// for the Phase 2 plan that fills in real cross-process control RPC.
enum LocalOrRemote {
    /// Same-process peer with an `Arc<MobHandle>` for direct dispatch.
    Local(MobHandle),
    /// Cross-process peer reachable via TCP/UDS.
    Remote(RemoteMobProxy),
}

/// Errors from cross-mob operations.
#[derive(Debug)]
pub enum CrossMobError {
    /// No contact directory configured on this runtime.
    NoContactDirectory,
    /// Mob ID not found in the contact directory.
    UnknownMob(String),
    /// No peer mob handle registered for this mob (required for inproc).
    NoPeerHandle(String),
    /// Caller asked to wire a non-inproc peer but did not supply (or
    /// could not derive) a 32-byte Ed25519 pubkey. Mobkit refuses to
    /// build an unsigned descriptor on real transports — meerkat-comms
    /// would then admit any sender at ingress.
    MissingPeerPubkey { mob_id: Option<String> },
    /// Member not found in the target mob's roster.
    MemberNotFound { member_id: String, mob_id: String },
    /// Member has no comms runtime (not comms-enabled).
    NoCommsInfo { member_id: String, mob_id: String },
    /// The underlying mob operation failed.
    Mob(meerkat_mob::MobError),
    /// Failed to build a trusted peer spec.
    PeerSpec(String),
    /// A cross-process control-channel call failed. Phase 1 returns this
    /// for any TCP/UDS contact entry that does not also have an
    /// in-process `MobHandle` registered — the seam is laid out, the
    /// real client lands in Phase 2.
    Remote(RemoteMobError),
}

impl std::fmt::Display for CrossMobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoContactDirectory => write!(f, "no contact directory configured"),
            Self::UnknownMob(id) => write!(f, "unknown mob: {id}"),
            Self::NoPeerHandle(id) => write!(f, "no peer mob handle registered for: {id}"),
            Self::Remote(e) => write!(f, "cross-process cross-mob: {e}"),
            Self::MemberNotFound { member_id, mob_id } => {
                write!(f, "member '{member_id}' not found in mob '{mob_id}'")
            }
            Self::NoCommsInfo { member_id, mob_id } => {
                write!(
                    f,
                    "member '{member_id}' in mob '{mob_id}' has no comms runtime"
                )
            }
            Self::Mob(err) => write!(f, "mob error: {err}"),
            Self::PeerSpec(reason) => write!(f, "peer spec error: {reason}"),
            Self::MissingPeerPubkey { mob_id } => match mob_id {
                Some(id) => write!(
                    f,
                    "non-inproc peer for mob '{id}' has no signing pubkey; \
                     bootstrap via mobkit/peer_pubkey or populate the contact \
                     directory's pubkey field before wiring"
                ),
                None => write!(
                    f,
                    "non-inproc peer has no signing pubkey; supply a 32-byte \
                     Ed25519 pubkey or use inproc transport"
                ),
            },
        }
    }
}

impl std::error::Error for CrossMobError {}

impl From<meerkat_mob::MobError> for CrossMobError {
    fn from(err: meerkat_mob::MobError) -> Self {
        Self::Mob(err)
    }
}

impl From<RemoteMobError> for CrossMobError {
    fn from(err: RemoteMobError) -> Self {
        Self::Remote(err)
    }
}

impl UnifiedRuntime {
    /// Register an external mob's handle for same-process cross-mob communication.
    pub async fn register_peer_mob(&self, mob_id: &str, handle: MobHandle) {
        self.peer_mob_handles
            .write()
            .await
            .insert(mob_id.to_string(), handle);
    }

    /// Set the contact directory for cross-mob address resolution.
    pub fn set_contact_directory(&mut self, directory: ContactDirectory) {
        self.contact_directory = Some(directory);
    }

    /// Install the long-lived Ed25519 keypair this gateway advertises via
    /// `mobkit/peer_pubkey` and (when meerkat-comms grows out-of-process
    /// transports) signs outbound envelopes with.
    ///
    /// Inproc-only deployments and most tests skip this — the in-process
    /// router authorises by identity map and signature verification is
    /// moot. Production gateways and any cross-process integration test
    /// must call this.
    pub fn set_gateway_peer_keys(&mut self, keys: GatewayPeerKeys) {
        self.gateway_peer_keys = Some(keys);
    }

    /// Borrow the local gateway keypair if one was installed.
    pub fn gateway_peer_keys(&self) -> Option<&GatewayPeerKeys> {
        self.gateway_peer_keys.as_ref()
    }

    /// Wire a local member to a member in an external mob.
    ///
    /// Resolves both members' peer IDs from roster entries, builds peer specs
    /// using the transport scheme advertised by the contact directory entry
    /// (`inproc`, `tcp`, or `uds`), and registers the peer on both sides to
    /// establish bidirectional trust.
    ///
    /// # Local vs remote dispatch
    ///
    /// The destination mob is dispatched as either [`LocalOrRemote::Local`]
    /// (when an `Arc<MobHandle>` was registered via [`Self::register_peer_mob`])
    /// or [`LocalOrRemote::Remote`] (when only a contact-directory TCP/UDS
    /// entry exists). Phase 1 ships the structural seam; Phase 2 wires the
    /// real cross-process control RPC — see
    /// `runtime::cross_mob_remote::RemoteMobProxy`.
    pub async fn wire_cross_mob(
        &self,
        local_member_id: &str,
        remote_member_id: &str,
        remote_mob_id: &str,
    ) -> Result<(), CrossMobError> {
        let entry = self.resolve_contact(remote_mob_id)?;
        let remote = self.dispatch_for(&entry).await?;

        // Phase 1: cross-process dispatch is a structural seam — short-
        // circuit before doing local-member lookups so callers see the
        // ControlChannelUnavailable surface unambiguously. Phase 2 will
        // run local lookup + open the control channel here.
        let remote_handle = match &remote {
            LocalOrRemote::Local(handle) => handle.clone(),
            LocalOrRemote::Remote(proxy) => {
                return Err(CrossMobError::Remote(
                    RemoteMobError::ControlChannelUnavailable {
                        mob_id: remote_mob_id.to_string(),
                        endpoint: proxy.endpoint().comms_address(),
                        operation: "wire_cross_mob",
                    },
                ));
            }
        };

        let local_handle = self.mob_runtime.handle();
        let local_mob_id = local_handle.mob_id().to_string();

        let local_mid = MeerkatId::from(local_member_id);
        let remote_mid = MeerkatId::from(remote_member_id);

        // Get peer info for both members from roster entries
        let (local_peer_id, local_comms_name) = self
            .get_member_peer_info(&local_handle, &local_mid, &local_mob_id)
            .await?;
        let (remote_peer_id, remote_comms_name) = self
            .get_member_peer_info(&remote_handle, &remote_mid, remote_mob_id)
            .await?;

        // Build peer specs honoring the contact-entry transport scheme.
        //
        // - `remote_spec` is what the **local** member uses to reach the
        //   remote: it carries the contact-entry transport (inproc/tcp/uds).
        // - `local_spec` is the back-pointer the **remote** member uses
        //   to reach us. The Local-handle dispatch resolves the local mob
        //   via the inproc registry, so we always advertise inproc for
        //   the local side here. (The Remote arm short-circuits above
        //   with ControlChannelUnavailable; Phase 2 will negotiate the
        //   advertised endpoint over the control channel.)
        let remote_spec = build_peer_spec(&remote_comms_name, &remote_peer_id, &entry.transport)?;
        let local_spec = build_peer_spec(&local_comms_name, &local_peer_id, &MobTransport::Inproc)?;

        // Wire the local side first.
        local_handle
            .wire(local_mid.clone(), PeerTarget::External(remote_spec))
            .await
            .map_err(CrossMobError::Mob)?;

        // Wire the remote side; roll back the local side on failure.
        if let Err(e) = remote_handle
            .wire(remote_mid.clone(), PeerTarget::External(local_spec))
            .await
        {
            // Best-effort rollback — leave local state consistent.
            if let Ok(rollback_spec) =
                build_peer_spec(&remote_comms_name, &remote_peer_id, &entry.transport)
            {
                let _ = local_handle
                    .unwire(local_mid, PeerTarget::External(rollback_spec))
                    .await;
            }
            return Err(CrossMobError::Mob(e));
        }

        Ok(())
    }

    /// Unwire a cross-mob peering.
    ///
    /// Best-effort on both sides — attempts to unwire both the local and
    /// remote members. Partial cleanup is better than aborting after one
    /// side fails, which would leave asymmetric peering.
    pub async fn unwire_cross_mob(
        &self,
        local_member_id: &str,
        remote_member_id: &str,
        remote_mob_id: &str,
    ) -> Result<(), CrossMobError> {
        let entry = self.resolve_contact(remote_mob_id)?;
        let remote = self.dispatch_for(&entry).await?;

        let remote_handle = match &remote {
            LocalOrRemote::Local(handle) => handle.clone(),
            LocalOrRemote::Remote(proxy) => {
                return Err(CrossMobError::Remote(
                    RemoteMobError::ControlChannelUnavailable {
                        mob_id: remote_mob_id.to_string(),
                        endpoint: proxy.endpoint().comms_address(),
                        operation: "unwire_cross_mob",
                    },
                ));
            }
        };

        let local_handle = self.mob_runtime.handle();
        let local_mob_id = local_handle.mob_id().to_string();

        let local_mid = MeerkatId::from(local_member_id);
        let remote_mid = MeerkatId::from(remote_member_id);

        let mut first_error: Option<CrossMobError> = None;

        // Unwire remote peer from local member.
        if let Ok((remote_peer_id, remote_comms_name)) = self
            .get_member_peer_info(&remote_handle, &remote_mid, remote_mob_id)
            .await
            && let Ok(spec) = build_peer_spec(&remote_comms_name, &remote_peer_id, &entry.transport)
            && let Err(e) = local_handle
                .unwire(local_mid.clone(), PeerTarget::External(spec))
                .await
        {
            first_error = Some(CrossMobError::Mob(e));
        }

        // Unwire local peer from remote member (always attempt, even if above failed)
        if let Ok((local_peer_id, local_comms_name)) = self
            .get_member_peer_info(&local_handle, &local_mid, &local_mob_id)
            .await
            && let Ok(spec) =
                build_peer_spec(&local_comms_name, &local_peer_id, &MobTransport::Inproc)
            && let Err(e) = remote_handle
                .unwire(remote_mid.clone(), PeerTarget::External(spec))
                .await
            && first_error.is_none()
        {
            first_error = Some(CrossMobError::Mob(e));
        }

        match first_error {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    /// Inject a message into a remote mob member's session.
    ///
    /// This is an **app-level injection** — the remote agent receives the
    /// message as an external turn but does not know who sent it. For
    /// agent-to-agent communication with sender identity and reply path,
    /// use `wire_cross_mob` to set up peering, then agents communicate
    /// directly via their comms `send` tool.
    ///
    /// `from_local_member` is recorded for audit/logging but does not
    /// affect delivery — the message is injected via the remote mob handle.
    pub async fn send_cross_mob(
        &self,
        from_local_member: &str,
        remote_member_id: &str,
        remote_mob_id: &str,
        content: impl Into<meerkat_core::ContentInput>,
    ) -> Result<String, CrossMobError> {
        let entry = self.resolve_contact(remote_mob_id)?;
        let remote = self.dispatch_for(&entry).await?;
        let remote_mid = MeerkatId::from(remote_member_id);
        let content = content.into();
        let _ = from_local_member; // audit context; delivery is via remote handle

        match remote {
            LocalOrRemote::Local(remote_handle) => {
                let _receipt = remote_handle
                    .member(&remote_mid)
                    .await
                    .map_err(CrossMobError::Mob)?
                    .send(content, HandlingMode::Queue)
                    .await
                    .map_err(CrossMobError::Mob)?;
                // Meerkat 0.6: MemberDeliveryReceipt no longer carries
                // session_id. Resolve the bridge session id from the
                // remote mob handle.
                let session_id = remote_handle
                    .resolve_bridge_session_id(&remote_mid)
                    .await
                    .ok_or_else(|| CrossMobError::NoCommsInfo {
                        member_id: remote_mid.to_string(),
                        mob_id: remote_mob_id.to_string(),
                    })?;
                Ok(session_id.to_string())
            }
            LocalOrRemote::Remote(proxy) => {
                // Phase 2 will serialize `content` and ship it over the
                // remote control channel. The Phase 1 stub returns
                // ControlChannelUnavailable before touching the payload,
                // so we pass `Null` and let the seam do the talking.
                let session_id = proxy
                    .inject_message(remote_member_id, serde_json::Value::Null)
                    .await?;
                Ok(session_id)
            }
        }
    }

    /// List external mobs from the contact directory.
    pub fn list_external_mobs(&self) -> Vec<ContactEntry> {
        self.contact_directory
            .as_ref()
            .map(|d| d.list().into_iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Whether a contact directory is configured (cross-mob operations available).
    pub fn has_contact_directory(&self) -> bool {
        self.contact_directory.is_some()
    }

    /// Whether any peer mob handles are registered (required for
    /// high-level cross-mob wire/unwire/send).
    pub async fn has_peer_mob_handles(&self) -> bool {
        !self.peer_mob_handles.read().await.is_empty()
    }

    /// Whether the contact directory has any inproc entries.
    pub fn has_inproc_contacts(&self) -> bool {
        self.contact_directory.as_ref().is_some_and(|d| {
            d.list()
                .iter()
                .any(|e| matches!(e.transport, MobTransport::Inproc))
        })
    }

    /// Whether the contact directory has any cross-process (TCP/UDS) entries.
    /// Useful for opting into the remote-mob proxy code path.
    pub fn has_remote_contacts(&self) -> bool {
        self.contact_directory.as_ref().is_some_and(|d| {
            d.list()
                .iter()
                .any(|e| matches!(e.transport, MobTransport::Tcp(_) | MobTransport::Uds(_)))
        })
    }

    /// Return the local mob's ID.
    pub fn mob_id(&self) -> String {
        self.mob_runtime.handle().mob_id().to_string()
    }

    /// Get comms peer info for a local member.
    /// Returns `(peer_id, comms_name, address)` — the address is always
    /// `inproc://{comms_name}` for local members. For cross-process peering,
    /// the caller should replace the address with the remote gateway's
    /// TCP/UDS endpoint.
    pub async fn local_member_peer_info(
        &self,
        member_id: &str,
    ) -> Result<(String, String, String), CrossMobError> {
        let handle = self.mob_runtime.handle();
        let mob_id = handle.mob_id().to_string();
        let mid = MeerkatId::from(member_id);
        let (peer_id, comms_name) = self.get_member_peer_info(&handle, &mid, &mob_id).await?;
        let address = format!("inproc://{comms_name}");
        Ok((peer_id, comms_name, address))
    }

    /// Wire a local member to an external peer using provided comms info.
    /// Only wires the local side — for the bidirectional wire, call this
    /// on both gateways.
    ///
    /// `remote_address` is the comms transport address (e.g. `"inproc://name"`
    /// for same-process, `"tcp://host:port"` for cross-process).
    /// `remote_pubkey` is the peer gateway's 32-byte Ed25519 verifying
    /// key. Inproc transports may pass `None` (the in-process router
    /// authorises by identity map). Non-inproc transports MUST supply a
    /// non-zero pubkey — this call fails closed with
    /// [`CrossMobError::MissingPeerPubkey`] otherwise so unsigned
    /// descriptors never reach a real transport.
    pub async fn wire_local(
        &self,
        local_member_id: &str,
        remote_comms_name: &str,
        remote_peer_id: &str,
        remote_address: &str,
        remote_pubkey: Option<[u8; 32]>,
    ) -> Result<(), CrossMobError> {
        let spec = build_external_peer_spec(
            remote_comms_name,
            remote_peer_id,
            remote_address,
            remote_pubkey,
        )?;
        let local_mid = MeerkatId::from(local_member_id);
        self.mob_runtime
            .handle()
            .wire(local_mid, PeerTarget::External(spec))
            .await
            .map_err(CrossMobError::Mob)
    }

    /// Undo a `wire_local` — unwire a local member from a previously wired peer.
    /// Only affects the local side; the remote side is left unchanged.
    pub async fn unwire_local(
        &self,
        local_member_id: &str,
        remote_comms_name: &str,
        remote_peer_id: &str,
        remote_address: &str,
        remote_pubkey: Option<[u8; 32]>,
    ) -> Result<(), CrossMobError> {
        let spec = build_external_peer_spec(
            remote_comms_name,
            remote_peer_id,
            remote_address,
            remote_pubkey,
        )?;
        let local_mid = MeerkatId::from(local_member_id);
        self.mob_runtime
            .handle()
            .unwire(local_mid, PeerTarget::External(spec))
            .await
            .map_err(CrossMobError::Mob)
    }

    // -- internal helpers --

    fn resolve_contact(&self, mob_id: &str) -> Result<ContactEntry, CrossMobError> {
        let dir = self
            .contact_directory
            .as_ref()
            .ok_or(CrossMobError::NoContactDirectory)?;
        dir.get(mob_id)
            .cloned()
            .ok_or_else(|| CrossMobError::UnknownMob(mob_id.to_string()))
    }

    /// Pick the appropriate dispatch arm for a contact entry.
    ///
    /// Order of preference:
    /// 1. **Local** — if a `MobHandle` was registered via
    ///    [`Self::register_peer_mob`] for this mob_id, use it directly
    ///    (covers the inproc + same-process-test paths).
    /// 2. **Remote** — for TCP/UDS contact entries with no registered
    ///    handle, build a [`RemoteMobProxy`].
    /// 3. **Error** — inproc entry with no registered handle, or unknown
    ///    transport.
    async fn dispatch_for(&self, entry: &ContactEntry) -> Result<LocalOrRemote, CrossMobError> {
        if let Some(handle) = self
            .peer_mob_handles
            .read()
            .await
            .get(&entry.mob_id)
            .cloned()
        {
            return Ok(LocalOrRemote::Local(handle));
        }
        match RemoteMobProxy::from_entry(entry)? {
            Some(proxy) => Ok(LocalOrRemote::Remote(proxy)),
            None => Err(CrossMobError::NoPeerHandle(entry.mob_id.clone())),
        }
    }

    /// Resolve a member's peer_id and comms name from the roster entry.
    ///
    /// Returns `(peer_id, comms_name)` where comms_name is derived as
    /// `"{mob_id}/{profile}/{meerkat_id}"` — this matches the canonical
    /// format used by meerkat-mob's `derived_comms_name()` and
    /// `build_agent_config()`. If meerkat-mob changes this format,
    /// this must be updated to match.
    async fn get_member_peer_info(
        &self,
        handle: &MobHandle,
        meerkat_id: &MeerkatId,
        mob_id: &str,
    ) -> Result<(String, String), CrossMobError> {
        let entry =
            handle
                .get_member(meerkat_id)
                .await
                .ok_or_else(|| CrossMobError::MemberNotFound {
                    member_id: meerkat_id.to_string(),
                    mob_id: mob_id.to_string(),
                })?;
        let peer_id = entry
            .peer_id()
            .ok_or_else(|| CrossMobError::NoCommsInfo {
                member_id: meerkat_id.to_string(),
                mob_id: mob_id.to_string(),
            })?
            .to_string();
        let comms_name = format!("{}/{}/{}", mob_id, entry.role, meerkat_id);
        Ok((peer_id, comms_name))
    }
}

/// Build a `TrustedPeerDescriptor` whose address reflects the supplied transport.
///
/// Uses the comms-layer address schemes:
/// - `inproc://{comms_name}` for in-process peers
/// - `tcp://host:port` for TCP
/// - `uds:///path` for UDS (note the triple slash — `uds://` plus an
///   absolute path)
///
/// **Phase-1 seam.** Until Unit 4 lands signed `TrustedPeerDescriptor`s
/// for cross-process peers, these specs go through
/// [`TrustedPeerDescriptor::test_only_unsigned`]. The in-process router
/// authorizes inproc peers via its identity map regardless; out-of-process
/// callers wait for Unit 4 to swap in a signed descriptor.
fn build_peer_spec(
    comms_name: &str,
    peer_id: &str,
    transport: &MobTransport,
) -> Result<TrustedPeerDescriptor, CrossMobError> {
    let address = match transport {
        MobTransport::Inproc => format!("inproc://{comms_name}"),
        MobTransport::Tcp(addr) => format!("tcp://{addr}"),
        MobTransport::Uds(path) => format!("uds://{path}"),
    };
    TrustedPeerDescriptor::test_only_unsigned(comms_name, peer_id, address)
        .map_err(CrossMobError::PeerSpec)
}

/// Build a [`TrustedPeerDescriptor`] for an external (non-inproc) peer.
///
/// The address scheme decides the policy:
///
/// * `inproc://...` — keep behaviour aligned with
///   [`build_inproc_peer_spec`]: pubkey is optional and an unsigned
///   descriptor is acceptable.
/// * `tcp://...` / `uds://...` — fail closed unless a non-zero 32-byte
///   pubkey is supplied. meerkat-comms keys its trust store by pubkey;
///   admitting an all-zero pubkey would let any sender in.
fn build_external_peer_spec(
    comms_name: &str,
    peer_id: &str,
    address: &str,
    pubkey: Option<[u8; 32]>,
) -> Result<TrustedPeerDescriptor, CrossMobError> {
    let is_inproc = address.starts_with("inproc://");
    match (is_inproc, pubkey) {
        (true, None) => TrustedPeerDescriptor::test_only_unsigned(comms_name, peer_id, address)
            .map_err(CrossMobError::PeerSpec),
        (true, Some(bytes)) => {
            TrustedPeerDescriptor::unsigned_with_pubkey(comms_name, peer_id, bytes, address)
                .map_err(CrossMobError::PeerSpec)
        }
        (false, None) => Err(CrossMobError::MissingPeerPubkey { mob_id: None }),
        (false, Some(bytes)) => {
            if bytes == [0u8; 32] {
                return Err(CrossMobError::MissingPeerPubkey { mob_id: None });
            }
            TrustedPeerDescriptor::unsigned_with_pubkey(comms_name, peer_id, bytes, address)
                .map_err(CrossMobError::PeerSpec)
        }
    }
}

/// Build a TCP peer descriptor.
///
/// Uses the comms-layer address scheme `tcp://host:port`. **Phase-1 seam**:
/// goes through [`TrustedPeerDescriptor::test_only_unsigned`]. Callers
/// that need a real signed descriptor (Ed25519-stamped) should construct
/// it via [`build_external_peer_spec`] with an explicit pubkey instead.
pub fn build_tcp_peer_spec(
    comms_name: &str,
    peer_id: &str,
    address: &str,
) -> Result<TrustedPeerDescriptor, CrossMobError> {
    TrustedPeerDescriptor::test_only_unsigned(comms_name, peer_id, format!("tcp://{address}"))
        .map_err(CrossMobError::PeerSpec)
}

/// Build a UDS peer descriptor.
///
/// Uses the comms-layer address scheme `uds:///path` (triple slash —
/// `uds://` + absolute path). See [`build_tcp_peer_spec`] for the
/// Phase-1 vs signed-descriptor seam note.
pub fn build_uds_peer_spec(
    comms_name: &str,
    peer_id: &str,
    path: &str,
) -> Result<TrustedPeerDescriptor, CrossMobError> {
    let normalized = if let Some(stripped) = path.strip_prefix('/') {
        stripped
    } else {
        path
    };
    TrustedPeerDescriptor::test_only_unsigned(comms_name, peer_id, format!("uds:///{normalized}"))
        .map_err(CrossMobError::PeerSpec)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    const TEST_PEER_ID: &str = "00000000-0000-4000-8000-000000000001";

    #[test]
    fn peer_spec_inproc_uses_comms_name_address() {
        let spec = build_peer_spec(
            "authors/coordinator/alice",
            TEST_PEER_ID,
            &MobTransport::Inproc,
        )
        .expect("spec");
        assert_eq!(spec.address.endpoint(), "authors/coordinator/alice");
    }

    #[test]
    fn peer_spec_tcp_uses_tcp_scheme() {
        let spec = build_peer_spec(
            "authors/coordinator/alice",
            "00000000-0000-4000-8000-000000000001",
            &MobTransport::Tcp("127.0.0.1:9001".to_string()),
        )
        .expect("spec");
        assert_eq!(spec.address.endpoint(), "127.0.0.1:9001");
    }

    #[test]
    fn peer_spec_uds_uses_uds_scheme() {
        let spec = build_peer_spec(
            "authors/coordinator/alice",
            "00000000-0000-4000-8000-000000000001",
            &MobTransport::Uds("/tmp/x.sock".to_string()),
        )
        .expect("spec");
        assert_eq!(spec.address.endpoint(), "/tmp/x.sock");
    }

    #[test]
    fn build_uds_peer_spec_handles_leading_slash() {
        // Caller may pass the path with or without a leading slash; both
        // produce the canonical `uds:///path` form.
        let with = build_uds_peer_spec("a", "00000000-0000-4000-8000-000000000001", "/tmp/x.sock")
            .expect("spec");
        let without =
            build_uds_peer_spec("a", "00000000-0000-4000-8000-000000000001", "tmp/x.sock")
                .expect("spec");
        assert_eq!(with.address.endpoint(), "/tmp/x.sock");
        assert_eq!(without.address.endpoint(), "/tmp/x.sock");
    }
}
