// @flow-editor-core — the MobKit Flow Editor's headless controller plane.
//
// Pure projection/transition logic extracted from the legacy
// flow-editor/src/controller.js monolith, module by module. Framework-free:
// no DOM access at load time, no React. The shell bundles this package as
// the window.MobKitFlowCore IIFE and the (shrinking) controller residue
// destructures from it via a build-injected prelude.
export * from "./catalogs/hydration";
export * from "./contract/options";
export * from "./document/build-projection";
export * from "./domain/tool-skill-access";
export * from "./drafts/mob-settings";
export * from "./editors/agent-editor";
export * from "./editors/basic-editor";
export * from "./editors/graph-editor";
export * from "./flow/launch-modes";
export * from "./flow/reconcile";
export * from "./flow/step-tree";
export * from "./members/patches";
export * from "./registry/flow-registry";
export * from "./rpc/client";
export * from "./schema/field-edit";
export * from "./shared/constants";
export * from "./shared/normalize";
export * from "./shell/outcomes";
export * from "./source/view";
export * from "./studio/state";
export * from "./views/view-config";
