//! Declared-versus-resolved capability invariant.
//!
//! A mob profile DECLARES tool categories under `[profiles.<name>.tools]`.
//! meerkat-mob maps each declaration onto a typed
//! `meerkat_core::ToolCategoryOverride` (`build_agent_config`, meerkat 0.8.22
//! `meerkat-mob/src/build.rs:250-259`) and `AgentFactory::build_agent`
//! RESOLVES it against the dispatcher slots the composing host actually
//! filled. Most categories fail closed when the declaration cannot be
//! served, which is loud by construction:
//!
//! - `workgraph`: enabled without a grant or dispatcher returns
//!   `BuildAgentError::Config` (`meerkat/src/factory.rs:5391-5395`,
//!   `:5419-5425`), and mobkit always attaches a workgraph service (the
//!   ephemeral in-memory fallback included), so the declaration resolves.
//! - `memory`: an enabled declaration that cannot open its store returns
//!   `BuildAgentError::CapabilityUnavailable` (`factory.rs:6087-6096`). The
//!   one silent arm - `effective_memory_for_realm` dropping memory for a
//!   `RecoveryBackendKind::Memory` realm (`factory.rs:1011-1017`) - is
//!   unreachable from the mob path: `AgentBuildConfig.backend` is `None` on
//!   every meerkat-mob construction site and `RecoveryBackendKind` appears
//!   nowhere in meerkat-mob or mobkit.
//! - `comms`: `tools.comms = false` is rejected at
//!   `build_agent_config` (`meerkat-mob/src/build.rs:170-173`).
//! - `builtins` / `shell`: the profile declaration is authoritative
//!   (`ToolCategoryOverride::Enable` ignores the factory default) and
//!   `compose_builtin_tools` composes whatever is enabled.
//! - `image_generation`: mobkit ORs the definition scan into the capability
//!   flags before composing, so the machine slot is filled whenever any
//!   profile declares it (`mob_handle_runtime.rs:6052`, `:6334`, `:6700`).
//!
//! `schedule` is the exception, and the reason this module exists. The
//! meerkat factory composes the scheduler surface only when the slot is
//! filled:
//!
//! ```text
//! if effective_schedule && let Some(schedule_dispatcher) = build_config.schedule_tools.take()
//! ```
//!
//! (`meerkat/src/factory.rs:5342`). When `schedule_tools` is empty the
//! declaration evaporates with nothing but a `tracing::debug!` line, so a
//! profile that says `tools.schedule = true` builds happily with no
//! `meerkat_schedule_*` tools and no operator-visible signal. In mobkit the
//! slot is filled only by `attach_schedule_tools_with_store`, which runs only
//! when a schedule store was injected (`mob_handle_runtime.rs:6500`,
//! `:6799`).
//!
//! The remaining `ToolConfig` entries are named here so the exclusion list is
//! CLOSED, not merely long. `ToolConfig` has eleven fields
//! (`meerkat-mob/src/profile.rs:16-61`): `schedule` is the subject above, the
//! bullets above account for six more (`workgraph`, `memory`, `comms`,
//! `builtins`, `shell`, `image_generation`), and these four are the rest.
//!
//! - `mob`: the factory arm has exactly the same
//!   `if effective && let Some(..) = slot.take()` shape as `schedule`
//!   (`meerkat/src/factory.rs:5482`), but mobkit fills the slot
//!   unconditionally: `install_agent_mob_tools` ends in a bare
//!   `*slot.write() = Some(factory)` and runs on every constructor arm
//!   (`mob_handle_runtime.rs:1449-1451`, called at `:5816`, `:6156`, `:6595`,
//!   `:6891`). Declared and resolved cannot diverge here.
//! - `rust_bundles`: a name with no registered bundle is a hard
//!   `MobError::Internal` at compose time
//!   (`meerkat-mob/src/runtime/tools.rs:437-443`), so it fails closed.
//! - `mcp` / `mcp_servers`: `tools.mcp` genuinely IS silent - it is a free
//!   allowlist of host MCP source ids and "mismatched names simply produce no
//!   tools at compose time. No mob-level validation."
//!   (`meerkat-mob/src/validate.rs:192-195`). It is excluded anyway because
//!   the RESOLVED side is not observable at this seam:
//!   `UnifiedRuntimeBuilder` exposes no MCP or external-tools input at all,
//!   and the mob-wide provider is reachable only through
//!   `MobBootstrapSpec::with_default_external_tools_provider`. Comparing here
//!   could only guess.
//!
//! # Scope
//!
//! This comparison is **warning-only**. Parking a member whose declared
//! capability is absent needs a typed park/hold alternative that does not
//! exist in this release, so nothing here refuses a build.
//!
//! Only inline profiles can be compared: a `ProfileBinding::RealmRef`
//! resolves inside meerkat-mob at spawn time, so its declarations are
//! unknowable here. Skipped realm-refs are COUNTED and reported, which is
//! also the positive control that the walk ran at all.
//!
//! # Who reaches this
//!
//! `UnifiedRuntimeBuilder`'s definition path, and nothing else. Both
//! first-party gateway binaries compose a `MobBootstrapSpec` by hand and
//! enter through `UnifiedRuntime::bootstrap*`, which never calls
//! `resolve_mob_spec`; `bin/rpc_gateway.rs:8368` says so in-tree. The
//! audience is therefore library embedders (and this crate's tests) that
//! build from a `MobDefinition`. The gateways' own version of the same
//! condition - a schedule store that fails to open, leaving the slot empty -
//! is already an explicitly declared degraded storage slot rather than a
//! silent drop, so they are exposed to the condition but not to the silence.

use meerkat_mob::MobDefinition;

/// A profile-declared tool category this invariant can compare.
///
/// Deliberately narrow: a category belongs here only when a declaration can
/// go missing at resolve time WITHOUT a typed error. Every other category
/// already fails closed (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum DeclaredToolCategory {
    /// `[profiles.<name>.tools] schedule = true`.
    Schedule,
}

impl DeclaredToolCategory {
    /// Stable wire/log token for this category.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Schedule => "schedule",
        }
    }

    /// Why an unresolved declaration in this category is silent.
    #[must_use]
    pub const fn detail(self) -> &'static str {
        match self {
            Self::Schedule => {
                "the agent factory's schedule dispatcher slot is empty, so meerkat composes no \
                 meerkat_schedule_* surface and the declaration is silently inert"
            }
        }
    }

    /// The operator-actionable fix.
    #[must_use]
    pub const fn remediation(self) -> &'static str {
        match self {
            Self::Schedule => {
                "inject UnifiedRuntimeBuilder::schedule_store(..) (or a storage_provider() whose \
                 realm set supplies one), or set tools.schedule = false on the profile"
            }
        }
    }
}

impl std::fmt::Display for DeclaredToolCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The tool surface the composing host actually resolved.
///
/// Each field answers "did the composition fill the slot this category needs",
/// NOT "did some profile ask for it". The two are compared in
/// [`compare_declared_capabilities`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResolvedToolSurface {
    /// Whether the agent factory's schedule dispatcher slot gets filled.
    pub schedule_tools: bool,
}

impl ResolvedToolSurface {
    /// Record whether schedule tools resolve on this composition.
    #[must_use]
    pub fn with_schedule_tools(mut self, resolved: bool) -> Self {
        self.schedule_tools = resolved;
        self
    }
}

/// One profile declaration the composed runtime surface cannot serve.
///
/// The category carries its own explanation and remediation, so the gap holds
/// only what is not derivable from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredCapabilityGap {
    /// The declaring profile name, as spelled in the definition.
    pub profile: String,
    /// The category declared but not resolvable.
    pub category: DeclaredToolCategory,
}

/// Outcome of one declared-versus-resolved comparison.
///
/// `inline_profiles_compared` and `realm_ref_profiles_skipped` are not
/// decoration: an all-zero report means the definition had no profiles at
/// all, which is a different fact from "no gaps found".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CapabilityInvariantReport {
    /// Declarations the resolved surface cannot serve, in definition order.
    pub gaps: Vec<DeclaredCapabilityGap>,
    /// Inline profiles whose declarations were actually compared.
    pub inline_profiles_compared: usize,
    /// `ProfileBinding::RealmRef` profiles skipped as unresolvable here.
    pub realm_ref_profiles_skipped: usize,
}

impl CapabilityInvariantReport {
    /// Whether every compared declaration resolves.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.gaps.is_empty()
    }
}

/// Compare every inline profile's declared tool categories against the
/// resolved runtime surface.
///
/// Pure: no I/O, no logging, no refusal. [`warn_declared_capability_gaps`]
/// renders the result; a future member-parking policy consumes
/// [`CapabilityInvariantReport::gaps`] directly.
///
/// Iteration order follows `MobDefinition::profiles`, a `BTreeMap`, so the
/// gap list is deterministic.
#[must_use]
pub fn compare_declared_capabilities(
    definition: &MobDefinition,
    resolved: ResolvedToolSurface,
) -> CapabilityInvariantReport {
    let mut report = CapabilityInvariantReport::default();
    for (name, binding) in &definition.profiles {
        let Some(profile) = binding.as_inline() else {
            // A realm-ref resolves inside meerkat-mob at spawn time; its
            // declarations are not readable here, and guessing them would
            // manufacture false warnings on a working fleet.
            report.realm_ref_profiles_skipped += 1;
            continue;
        };
        report.inline_profiles_compared += 1;
        if profile.tools.schedule && !resolved.schedule_tools {
            report.gaps.push(DeclaredCapabilityGap {
                profile: name.as_str().to_string(),
                category: DeclaredToolCategory::Schedule,
            });
        }
    }
    report
}

/// Emit the operator-visible form of a comparison.
///
/// One `WARN` per gap (each names the profile, the category, why it is
/// silent, and the fix) plus one summary line that is emitted
/// UNCONDITIONALLY. The unconditional summary is deliberate: without it, "no
/// warnings" is indistinguishable from "the invariant never ran".
///
/// The summary rides at `WARN` when there are gaps and `INFO` when there are
/// none, so the counts - including the realm-ref profiles this comparison
/// could not read - travel with the warnings at whatever level a host that
/// filters mobkit to `warn` is already showing. A host that shows
/// `meerkat_mobkit=info` (the level both first-party gateway binaries pick by
/// default, `bin/mobkit_gateway.rs:54` and `bin/rpc_gateway.rs:26`, though
/// neither reaches this function - see the module docs) also sees the
/// clean-run summary. The message text is identical at both levels; only the
/// level differs, which is why the macro call is written twice (the level is
/// part of the macro name).
pub fn warn_declared_capability_gaps(mob_id: &str, report: &CapabilityInvariantReport) {
    for gap in &report.gaps {
        tracing::warn!(
            mob_id = %mob_id,
            profile = %gap.profile,
            category = %gap.category.as_str(),
            detail = %gap.category.detail(),
            remediation = %gap.category.remediation(),
            "profile declares a tool category the composed runtime surface cannot serve"
        );
    }
    if report.is_clean() {
        tracing::info!(
            mob_id = %mob_id,
            inline_profiles_compared = report.inline_profiles_compared,
            realm_ref_profiles_skipped = report.realm_ref_profiles_skipped,
            gaps = report.gaps.len(),
            "declared-versus-resolved capability invariant evaluated"
        );
    } else {
        tracing::warn!(
            mob_id = %mob_id,
            inline_profiles_compared = report.inline_profiles_compared,
            realm_ref_profiles_skipped = report.realm_ref_profiles_skipped,
            gaps = report.gaps.len(),
            "declared-versus-resolved capability invariant evaluated"
        );
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn definition_with_schedule_declarations() -> MobDefinition {
        MobDefinition::from_toml(
            r#"
[mob]
id = "capability-invariant-test"

[profiles.planner]
model = "gpt-5.5"
runtime_mode = "autonomous_host"

[profiles.planner.tools]
comms = true
schedule = true

[profiles.worker]
model = "gpt-5.5"
runtime_mode = "autonomous_host"

[profiles.worker.tools]
comms = true
"#,
        )
        .expect("definition parses")
    }

    /// The gate observes a TRANSITION, not a total: the same definition
    /// yields exactly one gap when the slot is empty and zero when it is
    /// filled. Either assertion alone could pass vacuously.
    #[test]
    fn schedule_declaration_gaps_only_when_the_slot_is_unfilled() {
        let definition = definition_with_schedule_declarations();

        let unresolved = compare_declared_capabilities(&definition, ResolvedToolSurface::default());
        assert_eq!(
            unresolved.gaps,
            vec![DeclaredCapabilityGap {
                profile: "planner".to_string(),
                category: DeclaredToolCategory::Schedule,
            }],
            "only the declaring profile may be reported"
        );
        assert_eq!(unresolved.inline_profiles_compared, 2);
        assert_eq!(unresolved.realm_ref_profiles_skipped, 0);
        assert!(!unresolved.is_clean());

        let resolved = compare_declared_capabilities(
            &definition,
            ResolvedToolSurface::default().with_schedule_tools(true),
        );
        assert!(
            resolved.is_clean(),
            "a filled schedule slot must clear the gap, got: {:?}",
            resolved.gaps
        );
        assert_eq!(
            resolved.inline_profiles_compared, 2,
            "the clean report must prove the walk still ran"
        );
    }

    /// Realm-refs are counted, never guessed. The inline count in the same
    /// report is the positive control that the walk reached the inline
    /// profiles too.
    #[test]
    fn realm_ref_profiles_are_skipped_and_counted() {
        let mut definition = definition_with_schedule_declarations();
        definition.profiles.insert(
            meerkat_mob::ProfileName::from("remote"),
            meerkat_mob::ProfileBinding::RealmRef {
                realm_profile: "stored-profile".to_string(),
            },
        );

        let report = compare_declared_capabilities(&definition, ResolvedToolSurface::default());
        assert_eq!(report.realm_ref_profiles_skipped, 1);
        assert_eq!(report.inline_profiles_compared, 2);
        assert_eq!(
            report.gaps.len(),
            1,
            "the realm-ref must add no gap of its own"
        );
        assert!(
            report.gaps.iter().all(|gap| gap.profile != "remote"),
            "an unresolved realm-ref must never be reported as a gap"
        );
    }

    /// A definition with no profiles produces an all-zero report, which is a
    /// distinct fact from "compared some profiles and found nothing".
    #[test]
    fn empty_definition_reports_nothing_compared() {
        let definition = MobDefinition::from_toml("[mob]\nid = \"capability-invariant-empty\"\n")
            .expect("definition parses");
        let report = compare_declared_capabilities(&definition, ResolvedToolSurface::default());
        assert_eq!(report, CapabilityInvariantReport::default());
        assert!(report.is_clean());
    }

    /// The category's own strings are what the warning renders; keep them
    /// non-empty and self-describing.
    #[test]
    fn schedule_category_carries_detail_and_remediation() {
        let category = DeclaredToolCategory::Schedule;
        assert_eq!(category.as_str(), "schedule");
        assert_eq!(category.to_string(), "schedule");
        assert!(category.detail().contains("schedule"));
        assert!(
            category.remediation().contains("schedule_store"),
            "the remediation must name the builder seam that fills the slot"
        );
    }
}
