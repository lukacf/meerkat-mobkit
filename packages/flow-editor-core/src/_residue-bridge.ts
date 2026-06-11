// Temporary residue bridge — each wrapper is removed when its home slice
// lands (S17 catalogs/hydration).
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
// - flow/reconcile.ts deploy settings reconciliation/patches ->
//   deploySettingsForUi (residue until the S17 catalogs/hydration.ts slice).
//
// The S6 wrappers (contractDefaultValue, schemaFieldTypeOptions) were
// retired when contract/options.ts landed, the S8 wrappers
// (reconcileConditionFieldAvailability, reconcileSchemaFieldReferences)
// when flow/reconcile.ts landed, and the S11 wrappers (basicEditorViewState,
// graphCanvasViewState, graphCellXY) when the editors modules landed;
// importers now use the package modules relatively.
export function deploySettingsForUi(deploy: unknown) {
  return (window as any).MobKitFlowController.deploySettingsForUi(deploy);
}
