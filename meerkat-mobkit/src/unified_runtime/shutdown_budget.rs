//! The one authority for how long [`UnifiedRuntime::shutdown`] may take.
//!
//! Before this module, the advertised shutdown horizon was checked by a test that
//! summed a HAND-WRITTEN list of phases and compared it to a HAND-WRITTEN constant.
//! Both sides were maintained by the same hand, so the check could not notice a new
//! phase: PR #342 added a real bounded phase and the assertion stayed green until its
//! own term was added by hand.
//!
//! Here every bounded phase declares its budget in one place, the runtime timeout and
//! the advertised horizon are DERIVED from those declarations, and adding a phase is a
//! compile error until it declares one.
//!
//! [`UnifiedRuntime::shutdown`]: super::UnifiedRuntime::shutdown

use std::time::Duration;

/// A phase inside [`UnifiedRuntime::shutdown`] that is bounded by a timeout.
///
/// Only phases that SPEND time belong here. Slack does not: see
/// [`SCHEDULER_MARGIN`].
///
/// Adding a variant breaks [`ShutdownPhase::budget_secs`], which is an exhaustive match
/// with no wildcard. That compile error is the point - a phase cannot enter shutdown
/// without declaring what it may cost.
///
/// [`UnifiedRuntime::shutdown`]: super::UnifiedRuntime::shutdown
#[derive(Debug, Clone, Copy, PartialEq, Eq, strum::EnumIter)]
pub enum ShutdownPhase {
    /// Joining an identity operation that was already admitted when shutdown began.
    /// Bounded by the provider callback window, because shutdown must let the
    /// callback bridge answer before it can proceed.
    ProviderCallbackAdmittedOperation,
    /// The final batched lease release, which pays the same provider callback window
    /// a second time. One bound, spent twice serially, which is why this is a
    /// separate phase rather than a doubled constant.
    ProviderCallbackLeaseRelease,
    /// Draining runtime events. Bounded by the runtime's configured `drain_timeout`,
    /// whose default is derived from this phase.
    EventDrain,
    /// Waiting for the mob to quiesce before the final authority release.
    MobQuiesce,
    /// Joining the supervisor cleanups that replacement retired (PR #342). These are
    /// joined rather than aborted, so the budget is what bounds them.
    RetiredSupervisorJoin,
}

/// The provider callback window, spent once per provider-callback phase.
const PROVIDER_CALLBACK_SECS: u64 = 130;

impl ShutdownPhase {
    /// This phase's budget in whole seconds.
    ///
    /// Exhaustive on purpose: no wildcard arm, so a new variant cannot compile until
    /// somebody decides what it is allowed to cost.
    pub const fn budget_secs(self) -> u64 {
        match self {
            Self::ProviderCallbackAdmittedOperation => PROVIDER_CALLBACK_SECS,
            Self::ProviderCallbackLeaseRelease => PROVIDER_CALLBACK_SECS,
            Self::EventDrain => 30,
            Self::MobQuiesce => 10,
            Self::RetiredSupervisorJoin => 2,
        }
    }

    /// This phase's budget.
    #[must_use]
    pub const fn budget(self) -> Duration {
        Duration::from_secs(self.budget_secs())
    }
}

/// Every bounded phase, in the order shutdown spends them.
///
/// Membership is enforced by enumeration, not trusted: see
/// [`tests::every_variant_is_in_the_total`].
pub const BOUNDED_PHASES: &[ShutdownPhase] = &[
    ShutdownPhase::ProviderCallbackAdmittedOperation,
    ShutdownPhase::ProviderCallbackLeaseRelease,
    ShutdownPhase::EventDrain,
    ShutdownPhase::MobQuiesce,
    ShutdownPhase::RetiredSupervisorJoin,
];

/// Sum of every bounded phase's budget.
pub const BOUNDED_PHASE_TOTAL: Duration = {
    let mut secs = 0u64;
    let mut index = 0usize;
    while index < BOUNDED_PHASES.len() {
        secs += BOUNDED_PHASES[index].budget_secs();
        index += 1;
    }
    Duration::from_secs(secs)
};

/// Slack for task scheduling, NOT a phase.
///
/// Deliberately outside [`BOUNDED_PHASES`]: no code spends this, so modelling it as a
/// phase would send the next reader looking for the timeout that consumes it. It used
/// to exist only as a literal inside the horizon assertion, which meant the gate was
/// partly checking against a number the gate itself invented.
pub const SCHEDULER_MARGIN: Duration = Duration::from_secs(10);

/// How long the caller must allow [`UnifiedRuntime::shutdown`] to run.
///
/// Derived, never declared. Describes the DEFAULT configuration: `drain_timeout` is
/// builder-settable, so a host that overrides it makes [`ShutdownPhase::EventDrain`]
/// untrue for that runtime. The only overrides in this repo are two tests at 200ms.
///
/// [`UnifiedRuntime::shutdown`]: super::UnifiedRuntime::shutdown
pub const RUNTIME_SHUTDOWN_BUDGET: Duration =
    Duration::from_secs(BOUNDED_PHASE_TOTAL.as_secs() + SCHEDULER_MARGIN.as_secs());

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::*;

    /// Membership must be checked by ENUMERATING the variants, not by inspecting the
    /// list. Any guard that starts from `BOUNDED_PHASES` is blind to a variant that
    /// was never added to it: a length check sees an unchanged length, and an
    /// index-coverage check never sees the missing variant's index because it only
    /// reads indices of phases already present. Both were tried and both passed the
    /// mutation that adds an unlisted phase. `EnumIter` is what makes this real.
    #[test]
    fn every_variant_is_in_the_total() {
        for phase in ShutdownPhase::iter() {
            assert!(
                BOUNDED_PHASES.contains(&phase),
                "{phase:?} declares a budget but is absent from BOUNDED_PHASES, so it \
                 spends time that the advertised horizon does not cover"
            );
        }
        assert_eq!(
            BOUNDED_PHASES.len(),
            ShutdownPhase::iter().count(),
            "BOUNDED_PHASES has an entry that is not a variant, or a duplicate"
        );
    }

    /// The anchors. These are the numbers the gateway advertises and the SDKs wait on,
    /// so a change to any phase budget must be a deliberate edit here too.
    #[test]
    fn the_derived_budget_matches_the_advertised_numbers() {
        assert_eq!(BOUNDED_PHASE_TOTAL, Duration::from_secs(302));
        assert_eq!(SCHEDULER_MARGIN, Duration::from_secs(10));
        assert_eq!(RUNTIME_SHUTDOWN_BUDGET, Duration::from_secs(312));
        // Derived rather than restated: this is the property the old hand-written sum
        // was trying to express.
        assert_eq!(
            RUNTIME_SHUTDOWN_BUDGET,
            BOUNDED_PHASE_TOTAL + SCHEDULER_MARGIN
        );
    }

    #[test]
    fn one_provider_callback_bound_is_spent_twice() {
        assert_eq!(
            ShutdownPhase::ProviderCallbackAdmittedOperation.budget(),
            ShutdownPhase::ProviderCallbackLeaseRelease.budget(),
            "both provider-callback phases spend the same bound"
        );
    }
}
