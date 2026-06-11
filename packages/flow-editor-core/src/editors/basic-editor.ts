// Basic editor control plane for the Flow Editor. Seeded in S7 ahead of
// the S11 editors slice: basicBranchDefaultLabel is needed by
// drafts/mob-settings.ts (flowStepTemplate) and is facade-internal, so the
// lazy residue-bridge cannot reach it — it moved to its design-destined
// home early. Its basicEditorViewState dependency stays in the residue
// until S11 and goes through the bridge. The rest of the basic-editor
// cluster lands here in S11.
import { basicEditorViewState } from "../_residue-bridge";

export function basicBranchDefaultLabel(index, basicView = null) {
  const view = basicEditorViewState(basicView);
  const prefix = view.branchConditionRowTitlePrefix;
  return [prefix, String(index || 1)].filter(Boolean).join(" ");
}
