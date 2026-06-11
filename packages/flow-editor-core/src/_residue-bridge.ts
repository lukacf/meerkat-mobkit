// Temporary residue bridge — removed in S11 when the editors slices land.
//
// Package modules normally import each other relatively and never touch
// window or the facade. The ONE sanctioned exception (extraction-design
// risk 2) is a known straggler edge into a function that still lives in the
// controller.js residue: the wrapper defers to the facade lazily at CALL
// time (window.MobKitFlowController is assigned by the residue IIFE before
// any projection runs), so load order is never an issue. These wrappers are
// intentionally NOT re-exported through src/index.ts: the residue still
// declares the real functions, and a prelude entry with the same name would
// be a const/function redeclaration SyntaxError.
//
// Bridged edges:
// - domain/tool-skill-access.ts stepToolScopeState -> basicEditorViewState
//   (residue until the S11 editors/basic-editor.ts slice).
export function basicEditorViewState(basicView: unknown) {
  return (window as any).MobKitFlowController.basicEditorViewState(basicView);
}
