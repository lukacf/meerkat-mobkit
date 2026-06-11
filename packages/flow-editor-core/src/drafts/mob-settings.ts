// Mobpack drafts/settings helpers for the Flow Editor controller plane.
// Seeded in S5 ahead of the S7 drafts slice: editorSchemaFieldNameFallback
// is needed by schema/field-edit.ts (schemaFieldUpdatePatch) and is
// facade-internal, so the lazy residue-bridge cannot reach it — it moved to
// its design-destined home early along with its draft-contract chain
// (editorSchemaDraftContract, editorSchemaDraftField). editorSchemaDraftField
// calls schema/field-edit's schemaFieldName, a runtime-only import cycle with
// no module-init cross-calls; contractDefaultValue stays in the residue until
// S6 and goes through the lazy bridge. The rest of the drafts-mob-settings
// cluster lands here in S7.
import { schemaFieldName } from "../schema/field-edit";
import { contractDefaultValue } from "../_residue-bridge";

export function editorSchemaDraftField(rawField) {
  if (!rawField || typeof rawField !== "object") return null;
  const name = schemaFieldName(rawField.name, "");
  if (!name) return null;
  return {
    name,
    required: rawField.required === true,
    description: String(rawField.description || ""),
    enumValues: Array.isArray(rawField.enumValues)
      ? rawField.enumValues.map((value) => String(value || "").trim()).filter(Boolean)
      : [],
  };
}

export function editorSchemaDraftContract(contract) {
  const draft = contract?.mob_definition?.editor_schema_draft;
  if (!draft || typeof draft !== "object") return null;
  const schemaIdPrefix = String(draft.schema_id_prefix || "").trim();
  const schemaFieldType = contractDefaultValue(contract, "schema_field_type");
  const initialField = editorSchemaDraftField(draft.initial_field);
  const addedField = editorSchemaDraftField(draft.added_field);
  if (!schemaIdPrefix || !schemaFieldType || !initialField || !addedField) return null;
  return { schemaIdPrefix, schemaFieldType, initialField, addedField };
}

export function editorSchemaFieldNameFallback(contract) {
  const draft = editorSchemaDraftContract(contract);
  return draft?.addedField?.name || draft?.initialField?.name || "field";
}
