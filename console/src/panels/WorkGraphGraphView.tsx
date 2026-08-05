import React from "react";

import type { WorkGraphWireBinding, WorkGraphWireEdge, WorkGraphWireItem } from "../types";
import {
  layoutWorkGraph,
  workGraphEdgeMidpoint,
  workGraphEdgePath,
} from "../lib/workgraph-layout";
import type { WorkGraphLayoutNode } from "../lib/workgraph-layout";
import { useZoomPan, viewportTransform } from "./topology/zoom-pan";

/// Read-only layered-DAG rendering of the workgraph snapshot: nodes carry
/// the tree panel's status language, parent edges are solid arrows into the
/// parent, blocks edges dashed amber. Pan by dragging, zoom with the wheel
/// (non-passive listener inside useZoomPan so the dock scroll container
/// never eats it), Fit resets to the viewBox 1:1 fit. Mutations stay in the
/// tree/attention sections — the graph only selects.
interface WorkGraphGraphViewProps {
  items: WorkGraphWireItem[];
  edges: WorkGraphWireEdge[];
  attention: WorkGraphWireBinding[];
  selectedId?: string;
  onSelect?: (itemId: string) => void;
}

const TITLE_MAX_CHARS = 21;
const META_MAX_CHARS = 24;

function truncate(text: string, max: number): string {
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

function nodeMetaLine(node: WorkGraphLayoutNode): string {
  const parts: string[] = [];
  if (node.priority && node.priority !== "medium") parts.push(node.priority);
  if (node.blocked) parts.push("blocked");
  if (node.ownerLabel) parts.push(node.ownerLabel);
  if (node.alsoUnder.length > 0) parts.push(`also under ${node.alsoUnder.join(", ")}`);
  return truncate(parts.join(" · "), META_MAX_CHARS);
}

function nodeHoverText(node: WorkGraphLayoutNode, item: WorkGraphWireItem | undefined): string {
  const lines = [node.title, `status: ${node.status}`];
  if (node.ownerLabel) lines.push(`owner: ${node.ownerLabel}`);
  if (node.alsoUnder.length > 0) lines.push(`also under: ${node.alsoUnder.join(", ")}`);
  if (item?.description) lines.push(item.description);
  return lines.join("\n");
}

function selectionSummary(
  itemId: string,
  items: WorkGraphWireItem[],
  attention: WorkGraphWireBinding[],
): string {
  const item = items.find((candidate) => candidate.id === itemId);
  if (!item) return itemId;
  const parts = [itemId, item.status || "open"];
  const owner = item.owner?.display_name
    || item.owner?.key?.id
    || item.claim?.owner?.display_name
    || item.claim?.owner?.key?.id;
  if (owner) parts.push(owner);
  if (item.labels && item.labels.length > 0) parts.push(item.labels.join(", "));
  if (attention.some((binding) => binding.work_ref?.item_id === itemId)) parts.push("attention-bound");
  if (item.description) parts.push(item.description);
  return parts.join(" · ");
}

export function WorkGraphGraphView({
  items,
  edges,
  attention,
  selectedId,
  onSelect,
}: WorkGraphGraphViewProps): React.JSX.Element {
  const layout = React.useMemo(() => layoutWorkGraph(items, edges), [items, edges]);
  const zoom = useZoomPan(layout.width, layout.height);
  const boundItemIds = React.useMemo(() => {
    const bound = new Set<string>();
    for (const binding of attention) {
      const itemId = binding.work_ref?.item_id;
      if (typeof itemId === "string" && itemId) bound.add(itemId);
    }
    return bound;
  }, [attention]);

  if (layout.nodes.length === 0) {
    return <div className="workgraph__empty">No work items to draw.</div>;
  }

  return (
    <div className="workgraph-graph" data-testid="workgraph-graph-frame">
      <div className="workgraph-graph__toolbar">
        <button
          type="button"
          className="workgraph__action"
          data-testid="workgraph-graph-fit"
          onClick={zoom.reset}
        >
          Fit
        </button>
        <span className="workgraph-graph__stats">
          {layout.nodes.length} items · {layout.edges.length} edges
        </span>
        {layout.overflowCount > 0 ? (
          <span className="workgraph-graph__overflow" data-testid="workgraph-graph-overflow">
            +{layout.overflowCount} more items not drawn
          </span>
        ) : null}
        <span className="workgraph__spacer" />
        <span className="workgraph-graph__hint">drag to pan · wheel to zoom</span>
      </div>
      <svg
        data-testid="workgraph-graph"
        className={`workgraph-graph__svg${zoom.isDragging ? " is-dragging" : ""}`}
        viewBox={`0 0 ${layout.width} ${layout.height}`}
        preserveAspectRatio="xMidYMid meet"
        role="img"
        aria-label="Work item dependency graph"
        ref={zoom.svgRef}
        onPointerDown={zoom.onPointerDown}
        onPointerMove={zoom.onPointerMove}
        onPointerUp={zoom.onPointerUp}
        onPointerCancel={zoom.onPointerUp}
      >
        <defs>
          <marker
            id="workgraph-graph-arrow"
            className="workgraph-graph__arrow"
            viewBox="0 0 8 8"
            refX="7"
            refY="4"
            markerWidth="7"
            markerHeight="7"
            orient="auto-start-reverse"
          >
            <path d="M0,0 L8,4 L0,8 z" />
          </marker>
          <marker
            id="workgraph-graph-arrow-blocks"
            className="workgraph-graph__arrow is-blocks"
            viewBox="0 0 8 8"
            refX="7"
            refY="4"
            markerWidth="7"
            markerHeight="7"
            orient="auto-start-reverse"
          >
            <path d="M0,0 L8,4 L0,8 z" />
          </marker>
        </defs>
        <g data-testid="workgraph-graph-viewport" transform={viewportTransform(zoom.viewport)}>
          {layout.edges.map((edge, index) => {
            const mid = workGraphEdgeMidpoint(edge);
            const marker = edge.kind === "blocks"
              ? "url(#workgraph-graph-arrow-blocks)"
              : "url(#workgraph-graph-arrow)";
            return (
              <g key={`${edge.kind}:${edge.fromId}:${edge.toId}:${index}`}>
                <path
                  data-testid="workgraph-graph-edge"
                  data-kind={edge.kind}
                  className={`workgraph-graph__edge is-${edge.kind}`}
                  d={workGraphEdgePath(edge)}
                  markerEnd={marker}
                />
                {edge.kind !== "parent" ? (
                  <text
                    className="workgraph-graph__edge-label"
                    x={mid.x}
                    y={mid.y - 4}
                    textAnchor="middle"
                  >
                    {edge.kind}
                  </text>
                ) : null}
              </g>
            );
          })}
          {layout.nodes.map((node) => {
            const meta = nodeMetaLine(node);
            const selected = node.itemId === selectedId;
            return (
              <g
                key={node.itemId}
                data-testid="workgraph-graph-node"
                data-item-id={node.itemId}
                data-status={node.status}
                className={`workgraph-graph__node is-${node.status}${selected ? " is-selected" : ""}`}
                transform={`translate(${node.x} ${node.y})`}
                // Select on pointerdown, not click: the svg's pan handler
                // takes pointer capture, which retargets the eventual click
                // to the svg and would swallow node selection entirely.
                // stopPropagation keeps a node press from starting a pan.
                onPointerDown={(event) => {
                  event.stopPropagation();
                  onSelect?.(node.itemId);
                }}
              >
                <title>
                  {nodeHoverText(node, items.find((candidate) => candidate.id === node.itemId))}
                </title>
                <rect className="workgraph-graph__node-box" width={node.w} height={node.h} rx={8} />
                <circle className="workgraph-graph__node-dot" cx={14} cy={meta ? 15 : node.h / 2} r={3.5} />
                {boundItemIds.has(node.itemId) ? (
                  <circle
                    className="workgraph-graph__node-goal-ring"
                    cx={14}
                    cy={meta ? 15 : node.h / 2}
                    r={6.5}
                  />
                ) : null}
                <text
                  className="workgraph-graph__node-title"
                  x={26}
                  y={meta ? 19 : node.h / 2 + 4}
                >
                  {truncate(node.title, TITLE_MAX_CHARS)}
                </text>
                {meta ? (
                  <text className="workgraph-graph__node-meta" x={26} y={34}>
                    {meta}
                  </text>
                ) : null}
              </g>
            );
          })}
        </g>
      </svg>
      <div className="workgraph-graph__detail" data-testid="workgraph-graph-detail">
        {selectedId
          ? selectionSummary(selectedId, items, attention)
          : "Click a node to inspect it."}
      </div>
    </div>
  );
}

export const __workGraphGraphViewTest = {
  nodeMetaLine,
  selectionSummary,
};
