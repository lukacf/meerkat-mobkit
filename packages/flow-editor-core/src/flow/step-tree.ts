// Flow step-tree helpers for the Flow Editor controller plane. Seeded in
// S6 ahead of the S9 flow-step-tree slice: childLanes is needed by
// flow/launch-modes.ts (collectVisualSteps) and is facade-internal, so the
// lazy residue-bridge cannot reach it — it moved to its design-destined
// home early. The rest of the flow-step-tree cluster lands here in S9.

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

