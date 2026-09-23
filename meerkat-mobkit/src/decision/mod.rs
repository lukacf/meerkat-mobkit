//! MobKit composition of the Meerkat decision service.
//!
//! Meerkat's `meerkat-decision` crate owns the typed contracts, the shared
//! service, the backends, and the agent-callable `decide` tool. MobKit adds
//! the platform feature integrations the ADR calls for:
//!
//! - [`memory`]: applicability judgments between authorized candidate recall
//!   and the coordinator's final packing, with a declared baseline;
//! - [`work`]: per-requirement evidence, commitment applicability, rubric
//!   dimensions with non-compensating aggregation, and semantic fit among
//!   already-ready work;
//! - the `mobkit/decision/evaluate` JSON-RPC method (see `rpc`), so the
//!   Python and TypeScript SDKs consume the same service.
//!
//! None of these decide thresholds for applications beyond the policy they
//! declare in config, and none of them mint permission to act.

pub mod memory;
pub mod work;

pub use memory::{
    ApplicabilityAssessment, ApplicabilityBaseline, ApplicabilityDegradation, ApplicabilityOutcome,
    ExclusionReason, InclusionReason, MemoryApplicabilityConfig, MemoryApplicabilityConfigError,
    MemoryApplicabilityPolicy, RecordDisposition,
};
pub use work::{
    CommitmentApplicability, EvidenceItem, EvidenceRequirement, EvidenceSignal,
    RequirementEvidence, RubricAggregate, RubricDimension, RubricHardRule, RubricLevel,
    RubricScore, WorkCandidate, WorkDecisionError, WorkDecisionHelpers, WorkFit, aggregate_rubric,
    commitment_applicability_request, interpret_commitment_applicability,
    interpret_requirement_evidence, interpret_rubric, interpret_work_fit,
    requirement_evidence_request, rubric_request, work_fit_request,
};
