// Contract option helpers for the Flow Editor controller plane. Seeded in
// S5 ahead of the S6 contract-options slice: contractStringValues is needed
// by schema/field-edit.ts (schemaFieldTypeAllowedSet) and is facade-internal,
// so the lazy residue-bridge cannot reach it — it moved to its
// design-destined home early. The rest of the contract-options cluster lands
// here in S6.

export function contractStringValues(values) {
  return Array.isArray(values)
    ? values.map((value) => String(value || "").trim()).filter(Boolean)
    : [];
}
