//! MobKit's lowering of Meerkat's application tool-policy contract.
//!
//! Meerkat 0.8.26 defines the compiled artifact, the provider and snapshot
//! traits, and the registry that binds a member to a policy, but it ships no
//! production implementation of either trait: the only ones in the published
//! crate live inside its own test module. This module is that implementation and
//! nothing more.
//!
//! Two boundaries here are deliberate rather than incidental.
//!
//! ACCEPTED-REVISION OWNERSHIP IS OURS. `ToolConsequenceNarrowingPolicy`
//! documents that the provider owns the snapshot pointer and must reject a
//! revision below one already accepted, because Meerkat "deliberately keeps no
//! second accepted-revision store". So the monotonic check lives here, and it
//! refuses using Meerkat's own vocabulary, `RevisionRollback` and
//! `RevisionDigestConflict`, rather than inventing a local error for a condition
//! upstream already named.
//!
//! EXECUTION AUTHORITY IS THE GRANT, NOT THE CONSEQUENCE CLASS. In v1 a grant is
//! one exact allow entry with no wildcards and no deny entries, its only action
//! is `Invoke`, and neither `ToolConsequenceRequest` nor
//! `ToolConsequenceVerdict` carries a threshold. So an exact member/tool grant
//! allows, a miss defers to the artifact's own `default_deny`, and
//! `CompiledToolConsequence` R0-R3 is reported for observability WITHOUT gating
//! execution. Choosing a threshold here would make MobKit the authority on what
//! R2 means, which is upstream's to define if it is ever wanted.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use meerkat_core::{
    CompiledApplicationToolPolicy, CompiledMemberToolAction, PolicyDigest,
    PolicyEvaluationProvenance, PolicyId, PolicyProviderGeneration, PolicyProviderId,
    ToolConsequenceDenial, ToolConsequenceFailure, ToolConsequenceNarrowingPolicy,
    ToolConsequencePolicySnapshot, ToolConsequenceRequest, ToolConsequenceVerdict,
};

/// Denial code used when no exact grant covers the requested member and tool.
pub const DENIAL_CODE_NO_GRANT: &str = "application_tool_policy_no_grant";

/// One immutable compiled policy, served to Meerkat as a snapshot.
#[derive(Debug)]
pub struct CompiledPolicySnapshot {
    policy: CompiledApplicationToolPolicy,
}

impl CompiledPolicySnapshot {
    pub fn new(policy: CompiledApplicationToolPolicy) -> Self {
        Self { policy }
    }

    pub fn policy(&self) -> &CompiledApplicationToolPolicy {
        &self.policy
    }
}

impl ToolConsequencePolicySnapshot for CompiledPolicySnapshot {
    fn provenance(&self) -> PolicyEvaluationProvenance {
        PolicyEvaluationProvenance {
            revision: self.policy.revision,
            digest: self.policy.policy_digest.clone(),
        }
    }

    fn evaluate(&self, request: &ToolConsequenceRequest) -> ToolConsequenceVerdict {
        let grant = self
            .policy
            .members
            .iter()
            .find(|member| member.member_identity == request.member.member)
            .and_then(|member| {
                member
                    .grants
                    .iter()
                    .find(|grant| grant.tool_name == request.tool_name.as_str())
            });

        match grant {
            // The only action in v1 is `Invoke`, so an exact entry is the
            // decision. The consequence class travels in the observation, not
            // in the verdict.
            Some(grant) => match grant.action {
                CompiledMemberToolAction::Invoke => ToolConsequenceVerdict::Allow,
            },
            None if self.policy.default_deny => {
                ToolConsequenceVerdict::Deny(ToolConsequenceDenial::new(
                    DENIAL_CODE_NO_GRANT,
                    format!(
                        "no exact grant for tool '{}' and member '{}' in policy '{}' revision {}",
                        request.tool_name.as_str(),
                        request.member.member,
                        self.policy.policy_id,
                        self.policy.revision.0
                    ),
                ))
            }
            // `default_deny = false` is the artifact's own switch. Honouring it
            // is reading the policy, not authoring one.
            None => ToolConsequenceVerdict::Allow,
        }
    }
}

#[derive(Debug)]
struct AcceptedPolicy {
    revision: u64,
    digest: PolicyDigest,
    snapshot: Arc<CompiledPolicySnapshot>,
}

/// Serves the compiled policies MobKit was configured with, and owns the
/// accepted-revision fence Meerkat delegates to the provider.
#[derive(Debug)]
pub struct CompiledPolicyProvider {
    provider_id: PolicyProviderId,
    generation: PolicyProviderGeneration,
    accepted: RwLock<BTreeMap<PolicyId, AcceptedPolicy>>,
}

impl CompiledPolicyProvider {
    pub fn new(provider_id: PolicyProviderId, generation: PolicyProviderGeneration) -> Self {
        Self {
            provider_id,
            generation,
            accepted: RwLock::new(BTreeMap::new()),
        }
    }

    /// Install a compiled policy, refusing any revision that is not a forward
    /// move for its policy id.
    ///
    /// Rejects three things: a policy belonging to another provider, a revision
    /// below one already accepted, and a repeat of an accepted revision whose
    /// bytes differ. The last one matters because a silent content swap under a
    /// stable revision would make the digest the only evidence, and nothing
    /// downstream re-checks it.
    pub fn accept(
        &self,
        policy: CompiledApplicationToolPolicy,
    ) -> Result<(), ToolConsequenceFailure> {
        if policy.provider_id != self.provider_id {
            return Err(ToolConsequenceFailure::EvaluationFailed {
                reason: format!(
                    "compiled policy names provider '{}' but this provider is '{}'",
                    policy.provider_id, self.provider_id
                ),
            });
        }

        let mut accepted = self
            .accepted
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if let Some(current) = accepted.get(&policy.policy_id) {
            if policy.revision.0 < current.revision {
                return Err(ToolConsequenceFailure::RevisionRollback {
                    provider_id: self.provider_id.clone(),
                    policy_id: policy.policy_id.clone(),
                    accepted_revision: current.revision,
                    observed_revision: policy.revision.0,
                });
            }
            if policy.revision.0 == current.revision && policy.policy_digest != current.digest {
                return Err(ToolConsequenceFailure::RevisionDigestConflict {
                    provider_id: self.provider_id.clone(),
                    policy_id: policy.policy_id.clone(),
                    revision: policy.revision.0,
                });
            }
        }

        let policy_id = policy.policy_id.clone();
        let revision = policy.revision.0;
        let digest = policy.policy_digest.clone();
        accepted.insert(
            policy_id,
            AcceptedPolicy {
                revision,
                digest,
                snapshot: Arc::new(CompiledPolicySnapshot::new(policy)),
            },
        );
        Ok(())
    }

    /// Parse and install one canonical compiled-policy payload.
    ///
    /// Parsing goes through `parse_canonical_json`, so unknown fields, missing
    /// fields, non-canonical byte form and a mismatched digest all fail here
    /// rather than at first evaluation.
    pub fn accept_canonical_json(&self, bytes: &[u8]) -> Result<(), ToolConsequenceFailure> {
        let policy =
            CompiledApplicationToolPolicy::parse_canonical_json(bytes).map_err(|error| {
                ToolConsequenceFailure::EvaluationFailed {
                    reason: format!("compiled application tool policy rejected: {error}"),
                }
            })?;
        self.accept(policy)
    }

    /// Revision currently accepted for `policy_id`, for diagnostics.
    pub fn accepted_revision(&self, policy_id: &PolicyId) -> Option<u64> {
        self.accepted
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(policy_id)
            .map(|accepted| accepted.revision)
    }
}

impl ToolConsequenceNarrowingPolicy for CompiledPolicyProvider {
    fn provider_id(&self) -> &PolicyProviderId {
        &self.provider_id
    }

    fn generation(&self) -> PolicyProviderGeneration {
        self.generation
    }

    fn snapshot(
        &self,
        policy_id: &PolicyId,
    ) -> Result<Arc<dyn ToolConsequencePolicySnapshot>, ToolConsequenceFailure> {
        let accepted = self
            .accepted
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match accepted.get(policy_id) {
            Some(accepted) => Ok(Arc::clone(&accepted.snapshot) as Arc<_>),
            None => Err(ToolConsequenceFailure::PolicyMissing {
                provider_id: self.provider_id.clone(),
                policy_id: policy_id.clone(),
            }),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use meerkat_core::{
        CompiledMemberToolGrant, CompiledMemberToolGrants, CompiledPolicySourceProvenance,
        CompiledToolConsequence, MobMemberBinding, PolicyRevision, ToolName,
    };

    fn provider_id() -> PolicyProviderId {
        PolicyProviderId::new("mobkit-test-provider").expect("provider id")
    }

    fn policy_id() -> PolicyId {
        PolicyId::new("fleet-baseline").expect("policy id")
    }

    fn source() -> CompiledPolicySourceProvenance {
        CompiledPolicySourceProvenance {
            source_id: "mobkit-test-source".to_string(),
            source_digest: PolicyDigest::from_canonical_bytes(b"mobkit-test-source"),
        }
    }

    fn grants_for(member: &str, tool: &str) -> Vec<CompiledMemberToolGrants> {
        vec![CompiledMemberToolGrants {
            member_identity: member.to_string(),
            grants: vec![CompiledMemberToolGrant {
                tool_name: tool.to_string(),
                action: CompiledMemberToolAction::Invoke,
                consequence: CompiledToolConsequence::R2,
            }],
        }]
    }

    fn policy(revision: u64, member: &str, tool: &str) -> CompiledApplicationToolPolicy {
        CompiledApplicationToolPolicy::new(
            provider_id(),
            policy_id(),
            PolicyRevision(revision),
            source(),
            grants_for(member, tool),
        )
        .expect("compiled policy should validate")
    }

    fn request(member: &str, tool: &str) -> ToolConsequenceRequest {
        ToolConsequenceRequest {
            member: MobMemberBinding {
                mob_id: "mob".to_string(),
                role: "worker".to_string(),
                member: member.to_string(),
            },
            tool_name: ToolName::new(tool),
            arguments_json: "{}".to_string(),
            arguments_digest: "sha256:test".to_string(),
            run_id: None,
            tool_call_id: "call-1".to_string(),
            provider_id: provider_id(),
            policy_id: policy_id(),
        }
    }

    fn installed(policy: CompiledApplicationToolPolicy) -> Arc<CompiledPolicyProvider> {
        let provider = Arc::new(CompiledPolicyProvider::new(
            provider_id(),
            PolicyProviderGeneration(1),
        ));
        provider.accept(policy).expect("first accept");
        provider
    }

    #[test]
    fn an_exact_member_and_tool_grant_allows() {
        let provider = installed(policy(1, "member-a", "shell"));
        let snapshot = provider.snapshot(&policy_id()).expect("snapshot");
        assert!(matches!(
            snapshot.evaluate(&request("member-a", "shell")),
            ToolConsequenceVerdict::Allow
        ));
    }

    #[test]
    fn a_missing_grant_denies_under_the_artifacts_own_default_deny() {
        let policy = policy(1, "member-a", "shell");
        assert!(
            policy.default_deny,
            "a policy minted through new() must be default-deny"
        );
        let provider = installed(policy);
        let snapshot = provider.snapshot(&policy_id()).expect("snapshot");

        match snapshot.evaluate(&request("member-a", "network")) {
            ToolConsequenceVerdict::Deny(denial) => {
                assert_eq!(denial.code, DENIAL_CODE_NO_GRANT);
                assert!(denial.message.contains("network"), "{}", denial.message);
            }
            other => panic!("expected a typed denial, got {other:?}"),
        }
        // Same tool, wrong member: the grant is per identity, not global.
        assert!(matches!(
            snapshot.evaluate(&request("member-b", "shell")),
            ToolConsequenceVerdict::Deny(_)
        ));
    }

    #[test]
    fn the_provider_refuses_a_revision_rollback() {
        let provider = installed(policy(7, "member-a", "shell"));
        let error = provider
            .accept(policy(6, "member-a", "shell"))
            .expect_err("a lower revision must be refused");
        match error {
            ToolConsequenceFailure::RevisionRollback {
                accepted_revision,
                observed_revision,
                ..
            } => {
                assert_eq!(accepted_revision, 7);
                assert_eq!(observed_revision, 6);
            }
            other => panic!("expected RevisionRollback, got {other:?}"),
        }
        assert_eq!(provider.accepted_revision(&policy_id()), Some(7));
    }

    #[test]
    fn the_provider_refuses_a_reused_revision_whose_content_changed() {
        let provider = installed(policy(7, "member-a", "shell"));
        let error = provider
            .accept(policy(7, "member-a", "network"))
            .expect_err("same revision with different bytes must be refused");
        assert!(
            matches!(
                error,
                ToolConsequenceFailure::RevisionDigestConflict { revision: 7, .. }
            ),
            "expected RevisionDigestConflict, got {error:?}"
        );
        // The accepted snapshot must be unchanged, not partially replaced.
        let snapshot = provider.snapshot(&policy_id()).expect("snapshot");
        assert!(matches!(
            snapshot.evaluate(&request("member-a", "shell")),
            ToolConsequenceVerdict::Allow
        ));
    }

    #[test]
    fn a_forward_revision_replaces_the_snapshot() {
        let provider = installed(policy(1, "member-a", "shell"));
        provider
            .accept(policy(2, "member-a", "network"))
            .expect("a forward revision is accepted");
        assert_eq!(provider.accepted_revision(&policy_id()), Some(2));
        let snapshot = provider.snapshot(&policy_id()).expect("snapshot");
        assert!(matches!(
            snapshot.evaluate(&request("member-a", "network")),
            ToolConsequenceVerdict::Allow
        ));
        assert!(matches!(
            snapshot.evaluate(&request("member-a", "shell")),
            ToolConsequenceVerdict::Deny(_)
        ));
    }

    #[test]
    fn canonical_bytes_install_and_non_canonical_bytes_do_not() {
        let canonical = policy(3, "member-a", "shell")
            .canonical_json()
            .expect("canonical json");
        let provider = Arc::new(CompiledPolicyProvider::new(
            provider_id(),
            PolicyProviderGeneration(1),
        ));
        provider
            .accept_canonical_json(&canonical)
            .expect("canonical bytes install");
        assert_eq!(provider.accepted_revision(&policy_id()), Some(3));

        // Re-serialised without the canonical trailing newline: same policy,
        // non-canonical bytes, and the parse must refuse rather than normalise.
        let mut mangled = canonical;
        assert_eq!(mangled.pop(), Some(b'\n'));
        provider
            .accept_canonical_json(&mangled)
            .expect_err("non-canonical bytes must be refused");
    }

    #[test]
    fn an_unknown_policy_id_is_a_typed_miss() {
        let provider = installed(policy(1, "member-a", "shell"));
        let other = PolicyId::new("not-installed").expect("policy id");
        // `Arc<dyn Snapshot>` is not `Debug`, so `expect_err` cannot be used
        // here; match instead of loosening the trait upstream.
        let error = match provider.snapshot(&other) {
            Ok(_) => panic!("an unknown policy must not resolve"),
            Err(error) => error,
        };
        assert!(
            matches!(error, ToolConsequenceFailure::PolicyMissing { .. }),
            "expected PolicyMissing, got {error:?}"
        );
    }

    #[test]
    fn a_policy_from_another_provider_is_refused() {
        let provider = Arc::new(CompiledPolicyProvider::new(
            PolicyProviderId::new("mobkit-other").expect("provider id"),
            PolicyProviderGeneration(1),
        ));
        provider
            .accept(policy(1, "member-a", "shell"))
            .expect_err("a policy naming another provider must be refused");
    }
}
