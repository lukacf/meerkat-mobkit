// Document build projection for the Flow Editor controller plane. Seeded in
// S11 ahead of the S16 document/build-projection.ts slice:
// graphProjectionEdgeKinds is needed by editors/graph-editor.ts
// (graphEdgeCanvasState) and is facade-internal, so the lazy residue-bridge
// cannot reach it — it moved to its design-destined home early. The rest of
// the doc-build cluster (buildDocument, authoringDocumentFromState,
// graphProjectionForFlow, graphToFlow, the signature family) lands here in
// S16.
import { contractDefaultValue } from "../contract/options";

export function graphProjectionEdgeKinds(contract) {
  return {
    defaultKind: contractDefaultValue(contract, "graph_edge_kind"),
    conditionKind: contractDefaultValue(contract, "graph_condition_edge_kind"),
    fanoutKind: contractDefaultValue(contract, "graph_fanout_edge_kind"),
  };
}
