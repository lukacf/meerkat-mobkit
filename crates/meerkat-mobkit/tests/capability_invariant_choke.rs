//! Structural mutation gates for the declared-versus-resolved capability
//! invariant's first-party composition coverage.
//!
//! Behavioral fresh/resume and category-mutation tests live beside the
//! private wrapper implementation. These assertions make the production
//! gateway roots fail if either persistent or ephemeral composition bypasses
//! `MobBootstrapSpec::new`, or if the wrapper stops evaluating after the
//! inner service has materialized the live catalog.

#![allow(clippy::expect_used, clippy::panic)]

const RUNTIME_SOURCE: &str = include_str!("../src/mob_handle_runtime.rs");
const BUILDER_SOURCE: &str = include_str!("../src/unified_runtime/builder.rs");
const MOBKIT_GATEWAY_SOURCE: &str = include_str!("../src/bin/mobkit_gateway.rs");
const RPC_GATEWAY_SOURCE: &str = include_str!("../src/bin/rpc_gateway.rs");

fn occurrences(haystack: &str, needle: &str) -> usize {
    haystack.match_indices(needle).count()
}

#[test]
fn bootstrap_constructor_installs_the_post_materialization_wrapper() {
    let constructor = RUNTIME_SOURCE
        .split("impl MobBootstrapSpec {")
        .nth(1)
        .expect("MobBootstrapSpec impl")
        .split("pub fn dispatch_taint_slot")
        .next()
        .expect("constructor section");
    assert!(constructor.contains("PreBuildMobSessionService"));
    assert!(constructor.contains("inner: session_service"));

    let delegated_create = RUNTIME_SOURCE
        .split("macro_rules! delegate_mob_session_service")
        .nth(1)
        .expect("delegation macro")
        .split("async fn start_turn")
        .next()
        .expect("create_session implementation");
    let prepare = delegated_create
        .find("prepare_create_request")
        .expect("capture resolved declaration before materialization");
    let materialize = delegated_create
        .find("self.inner.create_session")
        .expect("inner materialization");
    let evaluate = delegated_create
        .find("complete_create")
        .expect("post-materialization evaluation");
    assert!(prepare < materialize && materialize < evaluate);
}

#[test]
fn both_gateways_funnel_persistent_and_ephemeral_roots_through_the_choke() {
    assert_eq!(
        occurrences(MOBKIT_GATEWAY_SOURCE, "MobBootstrapSpec::new("),
        2,
        "mobkit_gateway persistent and ephemeral roots must both use the common wrapper"
    );
    assert_eq!(
        occurrences(RPC_GATEWAY_SOURCE, "MobBootstrapSpec::new("),
        2,
        "rpc_gateway persistent and ephemeral roots must both use the common wrapper"
    );
}

#[test]
fn unified_builder_roots_funnel_through_wrapped_stock_constructors() {
    for constructor in [
        "MobBootstrapSpec::persistent_inner_with_provider_stores(",
        "MobBootstrapSpec::ephemeral_runtime_backed_with_provider_stores(",
        "MobBootstrapSpec::ephemeral_runtime_backed_inner(",
    ] {
        assert!(
            BUILDER_SOURCE.contains(constructor),
            "builder root must retain {constructor}"
        );
    }
    assert_eq!(
        occurrences(
            RUNTIME_SOURCE,
            "let mut spec = Self::new(definition, storage, session_service);"
        ),
        3,
        "every stock constructor used by the builder must install the common wrapper"
    );
}

#[test]
fn wrapper_does_not_skip_resume_materializations() {
    let delegation = RUNTIME_SOURCE
        .split("macro_rules! delegate_mob_session_service")
        .nth(1)
        .expect("delegation macro")
        .split("delegate_mob_session_service!(PreBuildMobSessionService)")
        .next()
        .expect("delegation implementation");
    assert!(
        !delegation.contains("resume_session.is_some"),
        "capability evaluation must not be gated out for either fresh or resumed requests"
    );
    assert_eq!(
        occurrences(delegation, "prepare_create_request(req).await?"),
        6,
        "ordinary, runtime-boundary, exact-witness, and archived-resume creation paths must all capture declared intent"
    );
    assert_eq!(
        occurrences(
            delegation,
            "complete_create(result, context, capability_context)"
        ),
        6,
        "every creation path must compare after successful materialization"
    );
}
