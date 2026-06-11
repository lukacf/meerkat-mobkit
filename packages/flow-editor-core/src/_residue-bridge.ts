// Temporary residue bridge — each wrapper is removed when its home slice
// lands (S11 editors, S17 catalogs/hydration).
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
// - domain/tool-skill-access.ts stepToolScopeState -> basicEditorViewState,
//   schema/field-edit.ts inputParamFieldControlState -> basicEditorViewState,
//   contract/options.ts basicStepPickerState -> basicEditorViewState,
//   editors/basic-editor.ts basicBranchDefaultLabel -> basicEditorViewState
//   (residue until the S11 editors/basic-editor.ts slice).
// - contract/options.ts graph option/menu builders -> graphCanvasViewState,
//   contract/options.ts graphAddMenuOpenProjection -> graphCellXY
//   (residue until the S11 editors/graph-editor.ts slice).
// - flow/reconcile.ts deploy settings reconciliation/patches ->
//   deploySettingsForUi (residue until the S17 catalogs/hydration.ts slice).
//
// The S6 wrappers (contractDefaultValue, schemaFieldTypeOptions) were
// retired when contract/options.ts landed, and the S8 wrappers
// (reconcileConditionFieldAvailability, reconcileSchemaFieldReferences)
// when flow/reconcile.ts landed; importers now use the package modules
// relatively.
export function basicEditorViewState(basicView: unknown) {
  return (window as any).MobKitFlowController.basicEditorViewState(basicView);
}

export function graphCanvasViewState(graphView: unknown) {
  return (window as any).MobKitFlowController.graphCanvasViewState(graphView);
}

export function graphCellXY(grid: unknown, col: unknown, row: unknown) {
  return (window as any).MobKitFlowController.graphCellXY(grid, col, row);
}

export function deploySettingsForUi(deploy: unknown) {
  return (window as any).MobKitFlowController.deploySettingsForUi(deploy);
}
