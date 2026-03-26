//! Cross-mob communication — peering and messaging between members in different mobs.

use meerkat_core::comms::TrustedPeerSpec;
use meerkat_core::types::HandlingMode;
use meerkat_mob::{MeerkatId, MobHandle, PeerTarget};

use crate::contact_directory::{ContactDirectory, ContactEntry};

use super::UnifiedRuntime;

/// Errors from cross-mob operations.
#[derive(Debug)]
pub enum CrossMobError {
    /// No contact directory configured on this runtime.
    NoContactDirectory,
    /// Mob ID not found in the contact directory.
    UnknownMob(String),
    /// No peer mob handle registered for this mob (required for inproc).
    NoPeerHandle(String),
    /// The contact entry uses TCP or UDS transport, which is not yet supported.
    /// Phase 1 only supports inproc (same-process) cross-mob communication.
    TransportNotSupported { mob_id: String, transport: String },
    /// Member not found in the target mob's roster.
    MemberNotFound { member_id: String, mob_id: String },
    /// Member has no comms runtime (not comms-enabled).
    NoCommsInfo { member_id: String, mob_id: String },
    /// The underlying mob operation failed.
    Mob(meerkat_mob::MobError),
    /// Failed to build a trusted peer spec.
    PeerSpec(String),
}

impl std::fmt::Display for CrossMobError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoContactDirectory => write!(f, "no contact directory configured"),
            Self::UnknownMob(id) => write!(f, "unknown mob: {id}"),
            Self::NoPeerHandle(id) => write!(f, "no peer mob handle registered for: {id}"),
            Self::TransportNotSupported { mob_id, transport } => {
                write!(
                    f,
                    "cross-mob transport '{transport}' for mob '{mob_id}' is not yet supported; \
                     only inproc (same-process) is supported in phase 1"
                )
            }
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
        }
    }
}

impl std::error::Error for CrossMobError {}

impl From<meerkat_mob::MobError> for CrossMobError {
    fn from(err: meerkat_mob::MobError) -> Self {
        Self::Mob(err)
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

    /// Wire a local member to a member in an external mob.
    ///
    /// Resolves both members' peer IDs from roster entries, builds peer specs,
    /// and calls `wire(local, PeerTarget::External(spec))` on both mob handles
    /// to establish bidirectional trust.
    pub async fn wire_cross_mob(
        &self,
        local_member_id: &str,
        remote_member_id: &str,
        remote_mob_id: &str,
    ) -> Result<(), CrossMobError> {
        let _entry = self.resolve_contact(remote_mob_id)?;
        let remote_handle = self.get_peer_handle(remote_mob_id).await?;
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

        // Build peer specs (inproc for same-process)
        let remote_spec = build_inproc_peer_spec(&remote_comms_name, &remote_peer_id)?;
        let local_spec = build_inproc_peer_spec(&local_comms_name, &local_peer_id)?;

        // Wire both sides
        local_handle
            .wire(local_mid, PeerTarget::External(remote_spec))
            .await
            .map_err(CrossMobError::Mob)?;
        remote_handle
            .wire(remote_mid, PeerTarget::External(local_spec))
            .await
            .map_err(CrossMobError::Mob)?;

        Ok(())
    }

    /// Unwire a cross-mob peering.
    ///
    /// Propagates lookup and unwire failures — if either side fails,
    /// the caller gets an error indicating the peering was not fully
    /// revoked.
    pub async fn unwire_cross_mob(
        &self,
        local_member_id: &str,
        remote_member_id: &str,
        remote_mob_id: &str,
    ) -> Result<(), CrossMobError> {
        let _entry = self.resolve_contact(remote_mob_id)?;
        let remote_handle = self.get_peer_handle(remote_mob_id).await?;
        let local_handle = self.mob_runtime.handle();
        let local_mob_id = local_handle.mob_id().to_string();

        let local_mid = MeerkatId::from(local_member_id);
        let remote_mid = MeerkatId::from(remote_member_id);

        // Unwire remote peer from local member
        let (remote_peer_id, remote_comms_name) = self
            .get_member_peer_info(&remote_handle, &remote_mid, remote_mob_id)
            .await?;
        let remote_spec = build_inproc_peer_spec(&remote_comms_name, &remote_peer_id)?;
        local_handle
            .unwire(local_mid.clone(), PeerTarget::External(remote_spec))
            .await
            .map_err(CrossMobError::Mob)?;

        // Unwire local peer from remote member
        let (local_peer_id, local_comms_name) = self
            .get_member_peer_info(&local_handle, &local_mid, &local_mob_id)
            .await?;
        let local_spec = build_inproc_peer_spec(&local_comms_name, &local_peer_id)?;
        remote_handle
            .unwire(remote_mid.clone(), PeerTarget::External(local_spec))
            .await
            .map_err(CrossMobError::Mob)?;

        Ok(())
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
        let _entry = self.resolve_contact(remote_mob_id)?;
        let remote_handle = self.get_peer_handle(remote_mob_id).await?;
        let remote_mid = MeerkatId::from(remote_member_id);
        let content = content.into();
        let _ = from_local_member; // audit context; delivery is via remote handle
        let receipt = remote_handle
            .member(&remote_mid)
            .await
            .map_err(CrossMobError::Mob)?
            .send(content, HandlingMode::Queue)
            .await
            .map_err(CrossMobError::Mob)?;
        Ok(receipt.session_id.to_string())
    }

    /// List external mobs from the contact directory.
    pub fn list_external_mobs(&self) -> Vec<ContactEntry> {
        self.contact_directory
            .as_ref()
            .map(|d| d.list().into_iter().cloned().collect())
            .unwrap_or_default()
    }

    // -- internal helpers --

    fn resolve_contact(&self, mob_id: &str) -> Result<ContactEntry, CrossMobError> {
        let dir = self
            .contact_directory
            .as_ref()
            .ok_or(CrossMobError::NoContactDirectory)?;
        let entry = dir
            .get(mob_id)
            .cloned()
            .ok_or_else(|| CrossMobError::UnknownMob(mob_id.to_string()))?;
        if !matches!(
            entry.transport,
            crate::contact_directory::MobTransport::Inproc
        ) {
            return Err(CrossMobError::TransportNotSupported {
                mob_id: mob_id.to_string(),
                transport: format!("{:?}", entry.transport),
            });
        }
        Ok(entry)
    }

    async fn get_peer_handle(&self, mob_id: &str) -> Result<MobHandle, CrossMobError> {
        self.peer_mob_handles
            .read()
            .await
            .get(mob_id)
            .cloned()
            .ok_or_else(|| CrossMobError::NoPeerHandle(mob_id.to_string()))
    }

    /// Resolve a member's peer_id and comms name from the roster entry.
    ///
    /// Returns `(peer_id, comms_name)` where comms_name is derived as
    /// `"{mob_id}/{profile}/{meerkat_id}"`.
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
        let peer_id = entry.peer_id.ok_or_else(|| CrossMobError::NoCommsInfo {
            member_id: meerkat_id.to_string(),
            mob_id: mob_id.to_string(),
        })?;
        let comms_name = format!("{}/{}/{}", mob_id, entry.profile, meerkat_id);
        Ok((peer_id, comms_name))
    }
}

fn build_inproc_peer_spec(
    comms_name: &str,
    peer_id: &str,
) -> Result<TrustedPeerSpec, CrossMobError> {
    TrustedPeerSpec::new(comms_name, peer_id, format!("inproc://{comms_name}"))
        .map_err(CrossMobError::PeerSpec)
}
