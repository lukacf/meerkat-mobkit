// Flow step-tree helpers for the Flow Editor controller plane. Seeded
// ahead of the S9 flow-step-tree slice: childLanes (S6) is needed by
// flow/launch-modes.ts (collectVisualSteps) and collectFlowStepIds (S7) by
// drafts/mob-settings.ts (uniqueFlowStepId); both are facade-internal, so
// the lazy residue-bridge cannot reach them — they moved to their
// design-destined home early, keeping original intra-cluster order
// (childLanes before collectFlowStepIds). The rest of the flow-step-tree
// cluster lands here in S9.

export function childLanes(step) {
  if (!step) return [];
  if (step.type === "repeat") return [{ id: "body", steps: step.steps || [] }];
  if (step.type === "branch") {
    return [
      ...(step.branches || []).map((branch) => ({ id: branch.id, steps: branch.steps || [] })),
      { id: "fallback", steps: step.fallback || [] },
    ];
  }
  if (step.type === "parallel") {
    return (step.branches || []).map((branch) => ({ id: branch.id, steps: branch.steps || [] }));
  }
  return [];
}

export function collectFlowStepIds(steps, out = new Set()) {
  for (const step of steps || []) {
    const id = String(step?.id || "").trim();
    if (id) out.add(id);
    for (const lane of childLanes(step || {})) collectFlowStepIds(lane.steps, out);
  }
  return out;
}
