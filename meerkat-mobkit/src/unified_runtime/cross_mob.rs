//! Cross-mob communication — peering and messaging between members in different mobs.

use meerkat_core::comms::TrustedPeerSpec;
use meerkat_mob::{MeerkatId, MemberCommsInfo, MobHandle};

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
    /// Resolves both members' comms info, builds peer specs, and calls
    /// `wire_external` on both mob handles to establish bidirectional trust.
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

        // Get comms info for both members
        let local_info = self
            .get_member_comms_info(&local_handle, &local_mid, &local_mob_id)
            .await?;
        let remote_info = self
            .get_member_comms_info(&remote_handle, &remote_mid, remote_mob_id)
            .await?;

        // Build peer specs (inproc for same-process)
        let remote_spec = build_inproc_peer_spec(&remote_info)?;
        let local_spec = build_inproc_peer_spec(&local_info)?;

        // Wire both sides
        local_handle
            .wire_external(local_mid, remote_spec)
            .await
            .map_err(CrossMobError::Mob)?;
        remote_handle
            .wire_external(remote_mid, local_spec)
            .await
            .map_err(CrossMobError::Mob)?;

        Ok(())
    }

    /// Unwire a cross-mob peering.
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

        // Get comms info for peer ID lookup
        if let Ok(remote_info) = self
            .get_member_comms_info(&remote_handle, &remote_mid, remote_mob_id)
            .await
        {
            let _ = local_handle
                .unwire_external(local_mid.clone(), remote_info.peer_id)
                .await;
        }
        if let Ok(local_info) = self
            .get_member_comms_info(&local_handle, &local_mid, &local_mob_id)
            .await
        {
            let _ = remote_handle
                .unwire_external(remote_mid.clone(), local_info.peer_id)
                .await;
        }

        Ok(())
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
        dir.get(mob_id)
            .cloned()
            .ok_or_else(|| CrossMobError::UnknownMob(mob_id.to_string()))
    }

    async fn get_peer_handle(&self, mob_id: &str) -> Result<MobHandle, CrossMobError> {
        self.peer_mob_handles
            .read()
            .await
            .get(mob_id)
            .cloned()
            .ok_or_else(|| CrossMobError::NoPeerHandle(mob_id.to_string()))
    }

    async fn get_member_comms_info(
        &self,
        handle: &MobHandle,
        meerkat_id: &MeerkatId,
        mob_id: &str,
    ) -> Result<MemberCommsInfo, CrossMobError> {
        handle
            .member_comms_info(meerkat_id.clone())
            .await
            .map_err(CrossMobError::Mob)?
            .ok_or_else(|| CrossMobError::NoCommsInfo {
                member_id: meerkat_id.to_string(),
                mob_id: mob_id.to_string(),
            })
    }
}

fn build_inproc_peer_spec(info: &MemberCommsInfo) -> Result<TrustedPeerSpec, CrossMobError> {
    TrustedPeerSpec::new(
        &info.comms_name,
        &info.peer_id,
        format!("inproc://{}", info.comms_name),
    )
    .map_err(CrossMobError::PeerSpec)
}
