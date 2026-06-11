// Temporary residue bridge — each wrapper is removed when its home slice
// lands (S6 contract/options, S8 flow/reconcile, S11 editors).
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
//   schema/field-edit.ts inputParamFieldControlState -> basicEditorViewState
//   (residue until the S11 editors/basic-editor.ts slice).
// - schema/field-edit.ts schemaLikeFieldTypeControlState ->
//   contractDefaultValue + schemaFieldTypeOptions, and
//   drafts/mob-settings.ts editorSchemaDraftContract -> contractDefaultValue
//   (residue until the S6 contract/options.ts slice).
// - schema/field-edit.ts schemaFieldUpdate/Rename/DeleteCascadePatch ->
//   reconcileConditionFieldAvailability + reconcileSchemaFieldReferences
//   (residue until the S8 flow/reconcile.ts slice).
export function basicEditorViewState(basicView: unknown) {
  return (window as any).MobKitFlowController.basicEditorViewState(basicView);
}

export function contractDefaultValue(contract: unknown, name: unknown) {
  return (window as any).MobKitFlowController.contractDefaultValue(contract, name);
}

export function schemaFieldTypeOptions(contract: unknown, currentType: unknown) {
  return (window as any).MobKitFlowController.schemaFieldTypeOptions(contract, currentType);
}

export function reconcileConditionFieldAvailability(spec: unknown) {
  return (window as any).MobKitFlowController.reconcileConditionFieldAvailability(spec);
}

export function reconcileSchemaFieldReferences(spec: unknown) {
  return (window as any).MobKitFlowController.reconcileSchemaFieldReferences(spec);
}
