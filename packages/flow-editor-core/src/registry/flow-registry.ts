// Flow-registry plane for the Flow Editor. Seeded in S12 ahead of the S15
// registry slice: normalizeAgentDefinitionRows is needed by
// editors/agent-editor.ts (sourceDefinitionRefRows) and is facade-internal,
// so the lazy residue-bridge cannot reach it — it moved to its
// design-destined home early. The rest of the flow-registry cluster lands
// here in S15.
export function normalizeAgentDefinitionRows(value) {
  if (!Array.isArray(value)) return [];
  return value
    .filter((row) => row && typeof row === "object" && !Array.isArray(row))
    .map((row) => JSON.parse(JSON.stringify(row)));
}
