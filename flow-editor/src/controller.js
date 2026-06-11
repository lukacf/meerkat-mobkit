/* global window */
// MobKit Flow Editor controller plane bootstrap.
// All controller logic lives in @flow-editor-core, bundled ahead of this
// file as window.MobKitFlowCore; this shim only constructs the
// window.MobKitFlowController facade the JSX views consume. The
// __MOBKIT_FLOW_CONTROLLER_TEST__ flag adds the test-gated assembler
// exports for the projection suite.
window.MobKitFlowController = window.MobKitFlowCore.createMobKitFlowController({
  includeTestExports: !!window.__MOBKIT_FLOW_CONTROLLER_TEST__,
});
