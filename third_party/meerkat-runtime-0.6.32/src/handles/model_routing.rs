//! Runtime impl of [`meerkat_core::handles::ModelRoutingHandle`].

use std::sync::Arc;

use meerkat_core::handles::{DslTransitionError, ModelRoutingHandle};
use meerkat_core::lifecycle::run_primitive::ModelId;

use super::HandleDslAuthority;
use crate::meerkat_machine::dsl as mm_dsl;

/// Runtime-backed [`ModelRoutingHandle`] impl.
#[derive(Debug)]
pub struct RuntimeModelRoutingHandle {
    dsl: Arc<HandleDslAuthority>,
}

impl RuntimeModelRoutingHandle {
    /// Construct a handle backed by the session's shared DSL authority.
    pub fn new(dsl: Arc<HandleDslAuthority>) -> Self {
        Self { dsl }
    }

    /// Construct a handle backed by an ephemeral DSL authority.
    pub fn ephemeral() -> Self {
        Self::new(Arc::new(HandleDslAuthority::ephemeral()))
    }
}

impl ModelRoutingHandle for RuntimeModelRoutingHandle {
    fn set_baseline(
        &self,
        baseline_model: ModelId,
        realtime_capable: bool,
    ) -> Result<(), DslTransitionError> {
        // intra-machine: no route; dispatcher not applicable (handle targets the meerkat DSL directly, not a CompositionDispatcher seam)
        self.dsl.apply_input(
            mm_dsl::MeerkatMachineInput::SetModelRoutingBaseline {
                baseline_model: baseline_model.to_string(),
                realtime_capable,
            },
            "ModelRoutingHandle::set_baseline",
        )
    }
}
