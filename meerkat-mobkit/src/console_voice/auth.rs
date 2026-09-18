//! Authenticated console ownership supplied to the shared Live admission owner.

use std::collections::{BTreeSet, HashMap};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use async_trait::async_trait;
use meerkat::experimental_gpt_live::{
    ExperimentalLiveOpenAuthorityError, ExperimentalLiveSessionBindingAuthority,
    ExperimentalLiveSessionBindingAuthorization,
};
use meerkat_core::{AuthBindingRef, SessionId};

use super::VoiceError;
use crate::access::AccessController;
use crate::live_wiring::{
    MobkitExperimentalLiveBindingUsePolicy, MobkitExperimentalLiveSessionBindingAuthority,
};

pub(crate) struct ConsoleLiveGrant {
    pub principal: String,
    pub identity: meerkat_mob::AgentIdentity,
    pub session: SessionId,
    pub revoked: AtomicBool,
}

#[derive(Default)]
struct GrantRegistry(Mutex<HashMap<SessionId, Weak<ConsoleLiveGrant>>>);

impl GrantRegistry {
    fn register(
        &self,
        principal: &str,
        identity: meerkat_mob::AgentIdentity,
        session: SessionId,
    ) -> Result<Arc<ConsoleLiveGrant>, VoiceError> {
        if principal.trim().is_empty() {
            return Err(VoiceError::Unauthorized);
        }
        let mut grants = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(existing) = grants.get(&session).and_then(Weak::upgrade) {
            // An active voice retains this grant. Exact-scope probes reuse it;
            // only the separate open-request fence enforces channel exclusivity.
            return if !existing.revoked.load(Ordering::SeqCst)
                && existing.principal == principal
                && existing.identity == identity
            {
                Ok(existing)
            } else {
                Err(VoiceError::Busy)
            };
        }
        grants.retain(|_, grant| grant.strong_count() > 0);
        let grant = Arc::new(ConsoleLiveGrant {
            principal: principal.to_string(),
            identity,
            session: session.clone(),
            revoked: AtomicBool::new(false),
        });
        grants.insert(session, Arc::downgrade(&grant));
        Ok(grant)
    }

    fn get(
        &self,
        session: &SessionId,
    ) -> Result<Arc<ConsoleLiveGrant>, ExperimentalLiveOpenAuthorityError> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session)
            .and_then(Weak::upgrade)
            .filter(|grant| !grant.revoked.load(Ordering::SeqCst))
            .ok_or(ExperimentalLiveOpenAuthorityError::DurableTargetUnavailable)
    }
}

pub(crate) struct ConsoleLiveBindingAuthority {
    handle: meerkat_mob::MobHandle,
    machine: Arc<meerkat_runtime::MeerkatMachine>,
    access: AccessController,
    binding: AuthBindingRef,
    grants: GrantRegistry,
}

impl ConsoleLiveBindingAuthority {
    pub(crate) fn new(
        handle: meerkat_mob::MobHandle,
        machine: Arc<meerkat_runtime::MeerkatMachine>,
        access: AccessController,
        binding: AuthBindingRef,
    ) -> Self {
        Self {
            handle,
            machine,
            access,
            binding,
            grants: GrantRegistry::default(),
        }
    }

    pub(crate) fn register(
        &self,
        principal: &str,
        identity: meerkat_mob::AgentIdentity,
        session: SessionId,
    ) -> Result<Arc<ConsoleLiveGrant>, VoiceError> {
        self.grants.register(principal, identity, session)
    }

    fn for_session(
        &self,
        session: &SessionId,
    ) -> Result<MobkitExperimentalLiveSessionBindingAuthority, ExperimentalLiveOpenAuthorityError>
    {
        let grant = self.grants.get(session)?;
        Ok(MobkitExperimentalLiveSessionBindingAuthority::new(
            self.handle.clone(),
            Arc::clone(&self.machine),
            grant.identity.clone(),
            self.access.view_for_subject(Some(&grant.principal)),
            Arc::new(ConsoleBindingPolicy {
                grant,
                binding: self.binding.clone(),
            }),
        ))
    }
}

#[async_trait]
impl ExperimentalLiveSessionBindingAuthority for ConsoleLiveBindingAuthority {
    async fn validate_live_durable_source_availability(
        &self,
        session: &SessionId,
    ) -> Result<(), ExperimentalLiveOpenAuthorityError> {
        self.for_session(session)?
            .validate_live_durable_source_availability(session)
            .await
    }

    async fn authorize_binding_use(
        &self,
        session: &SessionId,
        binding: &AuthBindingRef,
    ) -> Result<ExperimentalLiveSessionBindingAuthorization, ExperimentalLiveOpenAuthorityError>
    {
        self.for_session(session)?
            .authorize_binding_use(session, binding)
            .await
    }
}

struct ConsoleBindingPolicy {
    grant: Arc<ConsoleLiveGrant>,
    binding: AuthBindingRef,
}

#[async_trait]
impl MobkitExperimentalLiveBindingUsePolicy for ConsoleBindingPolicy {
    async fn authorize_binding_use(
        &self,
        session: &SessionId,
        binding: &AuthBindingRef,
    ) -> Result<meerkat_core::AuthBindingUseWitness, ExperimentalLiveOpenAuthorityError> {
        if session != &self.grant.session || binding != &self.binding {
            return Err(ExperimentalLiveOpenAuthorityError::BindingUseDenied);
        }
        let principal = meerkat_core::PrincipalRef::new(
            meerkat_core::PrincipalKind::Human,
            self.grant.principal.clone(),
        )
        .map_err(|_| ExperimentalLiveOpenAuthorityError::BindingUseDenied)?;
        let target = meerkat_core::PrincipalRef::new(
            meerkat_core::PrincipalKind::PersonalAgent,
            crate::member_comms_id::logical_memory_identity(self.grant.identity.as_str()),
        )
        .map_err(|_| ExperimentalLiveOpenAuthorityError::BindingUseDenied)?;
        let request = meerkat_core::AuthBindingUseRequest::new(
            principal.clone(),
            target.clone(),
            binding.clone(),
        );
        let policy = meerkat_core::AuthGrant {
            principal: principal.clone(),
            scope: meerkat_core::GrantScope::AuthBinding {
                realm_id: binding.realm.clone(),
                binding_id: binding.binding.clone(),
                profile_id: binding.profile.clone(),
            },
            actions: BTreeSet::from([meerkat_core::GrantAction::UseAuthBinding]),
            acting_on_behalf_of: Some(meerkat_core::ActingOnBehalfOf::new(principal, target)),
        };
        meerkat_core::authorize_explicit_auth_binding_use(&request, &[policy])
            .into_result()
            .map_err(|_| ExperimentalLiveOpenAuthorityError::BindingUseDenied)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn grants_are_exact_live_owner_scopes_not_principal_fallbacks() {
        let registry = GrantRegistry::default();
        let session = SessionId::new();
        let identity = meerkat_mob::AgentIdentity::from("agent-a");
        assert!(matches!(
            registry.register("", identity.clone(), session.clone()),
            Err(VoiceError::Unauthorized)
        ));
        let grant = registry
            .register("alice", identity.clone(), session.clone())
            .expect("owner");
        assert!(Arc::ptr_eq(
            &grant,
            &registry
                .register("alice", identity.clone(), session.clone())
                .expect("same owner")
        ));
        assert!(matches!(
            registry.register("bob", identity.clone(), session.clone()),
            Err(VoiceError::Busy)
        ));
        assert!(matches!(
            registry.register(
                "alice",
                meerkat_mob::AgentIdentity::from("other"),
                session.clone()
            ),
            Err(VoiceError::Busy)
        ));
        drop(grant);
        assert!(
            registry.get(&session).is_err(),
            "disposal revokes admission"
        );
        registry
            .register("bob", identity, session)
            .expect("new owner after disposal");
    }
}
