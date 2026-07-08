import React from "react";
import type {
  WorkGraphWireBinding,
  WorkGraphWireEdge,
  WorkGraphWireEvent,
  WorkGraphWireItem,
} from "../types";

/// Snapshot-backed panel state assembled by ConsoleApp. `denied` is a -32030
/// access outcome (rendered as "no grant", never as an empty store);
/// `unavailable` means no WorkGraph service is configured on the runtime.
export interface WorkGraphPanelData {
  items: WorkGraphWireItem[];
  edges: WorkGraphWireEdge[];
  attention: WorkGraphWireBinding[];
  events: WorkGraphWireEvent[];
  capturedAt: string | null;
  unavailable: boolean;
  denied: boolean;
  error: string | null;
}

interface WorkGraphPanelProps {
  data: WorkGraphPanelData;
  canManage: boolean;
  onRefresh: () => void;
  onClaim?: (input: { itemId: string; revision: number }) => void;
  onClose?: (input: { itemId: string; revision: number }) => void;
  onGoalConfirm?: (input: { bindingId: string; revision?: number }) => void;
  onGoalRequestClose?: (input: { bindingId: string; revision?: number }) => void;
  onAttentionPause?: (input: { bindingId: string; revision?: number }) => void;
  onAttentionResume?: (input: { bindingId: string; revision?: number }) => void;
  onAttentionReassign?: (input: { bindingId: string; revision?: number; identity: string }) => void;
}

export interface WorkGraphPanelTreeRow {
  item: WorkGraphWireItem;
  itemId: string;
  depth: number;
}

/// DFS order over parent edges (WorkEdge kind "parent" runs child→parent).
/// Roots first (by created_at, then id), children indented under parents;
/// items with a parent edge to an unknown item render as roots.
export function buildWorkGraphPanelTree(
  items: WorkGraphWireItem[],
  edges: WorkGraphWireEdge[],
): WorkGraphPanelTreeRow[] {
  const byId = new Map<string, WorkGraphWireItem>();
  for (const item of items) {
    if (typeof item.id === "string" && item.id) byId.set(item.id, item);
  }
  const parentOf = new Map<string, string>();
  for (const edge of edges) {
    if (edge.kind !== "parent") continue;
    if (
      typeof edge.from_id === "string" && edge.from_id
      && typeof edge.to_id === "string" && edge.to_id
      && edge.from_id !== edge.to_id
    ) {
      parentOf.set(edge.from_id, edge.to_id);
    }
  }
  const childrenOf = new Map<string, string[]>();
  const roots: string[] = [];
  for (const id of byId.keys()) {
    const parent = parentOf.get(id);
    if (parent && byId.has(parent)) {
      const children = childrenOf.get(parent) || [];
      children.push(id);
      childrenOf.set(parent, children);
    } else {
      roots.push(id);
    }
  }
  const sortIds = (ids: string[]): string[] => (
    [...ids].sort((left, right) => {
      const leftKey = byId.get(left)?.created_at || "";
      const rightKey = byId.get(right)?.created_at || "";
      if (leftKey !== rightKey) return leftKey < rightKey ? -1 : 1;
      return left < right ? -1 : left === right ? 0 : 1;
    })
  );
  const rows: WorkGraphPanelTreeRow[] = [];
  const visited = new Set<string>();
  const visit = (id: string, depth: number) => {
    if (visited.has(id)) return;
    visited.add(id);
    const item = byId.get(id);
    if (!item) return;
    rows.push({ item, itemId: id, depth });
    for (const child of sortIds(childrenOf.get(id) || [])) {
      visit(child, depth + 1);
    }
  };
  for (const root of sortIds(roots)) visit(root, 0);
  return rows;
}

export function workGraphBindingStatusLabel(binding: WorkGraphWireBinding): string {
  const state = binding.status?.state || "active";
  if (state === "paused") {
    const until = binding.status?.until;
    return until ? `paused until ${until.slice(0, 16).replace("T", " ")}` : "paused";
  }
  return state;
}

export function workGraphBindingTargetLabel(binding: WorkGraphWireBinding): string {
  const target = binding.target;
  if (!target) return "";
  if (typeof target.session_id === "string" && target.session_id) return target.session_id;
  const ownerKey = target.owner_key;
  if (ownerKey) {
    return [ownerKey.kind, ownerKey.id].filter(Boolean).join(":");
  }
  return "";
}

export function workGraphEventLine(event: WorkGraphWireEvent): string {
  const kind = typeof event.kind === "string" ? event.kind.replace(/_/g, " ") : "event";
  const at = typeof event.at === "string" && event.at.length >= 16
    ? `${event.at.slice(0, 10)} ${event.at.slice(11, 16)}`
    : "";
  const item = typeof event.item_id === "string" && event.item_id ? event.item_id : "";
  return [at, kind, item].filter(Boolean).join(" · ");
}

export function workGraphOwnerLabelOf(item: WorkGraphWireItem): string {
  return item.owner?.display_name
    || item.owner?.key?.id
    || item.claim?.owner?.display_name
    || item.claim?.owner?.key?.id
    || "";
}

export const __workGraphPanelTest = {
  buildWorkGraphPanelTree,
  workGraphBindingStatusLabel,
  workGraphBindingTargetLabel,
  workGraphEventLine,
  workGraphOwnerLabelOf,
};

function statusDotClass(status: string | undefined): string {
  return `workgraph__dot is-${status || "open"}`;
}

function ItemRow({
  row,
  canManage,
  onClaim,
  onClose,
}: {
  row: WorkGraphPanelTreeRow;
  canManage: boolean;
  onClaim?: WorkGraphPanelProps["onClaim"];
  onClose?: WorkGraphPanelProps["onClose"];
}) {
  const { item, itemId, depth } = row;
  const status = item.status || "open";
  const revision = typeof item.revision === "number" ? item.revision : 0;
  const terminal = status === "completed" || status === "cancelled" || status === "failed";
  const owner = workGraphOwnerLabelOf(item);
  return (
    <div
      className="workgraph__item"
      data-testid={`workgraph-panel-item:${itemId}`}
      style={{ paddingLeft: `${depth * 16}px` }}
    >
      <span className={statusDotClass(status)} aria-hidden="true" />
      <span className="workgraph__item-title" title={item.description || item.title}>
        {item.title || itemId}
      </span>
      {item.priority && item.priority !== "medium" ? (
        <span className={`workgraph__chip is-priority-${item.priority}`}>{item.priority}</span>
      ) : null}
      {owner ? <span className="workgraph__chip">{owner}</span> : null}
      <span className="workgraph__item-status">{status.replace(/_/g, " ")}</span>
      {canManage && onClaim && status === "open" && !owner ? (
        <button
          type="button"
          className="workgraph__action"
          data-testid={`workgraph-panel-action:${itemId}:claim`}
          onClick={() => onClaim({ itemId, revision })}
        >
          Claim
        </button>
      ) : null}
      {canManage && onClose && !terminal && status !== "blocked" ? (
        <button
          type="button"
          className="workgraph__action"
          data-testid={`workgraph-panel-action:${itemId}:close`}
          onClick={() => onClose({ itemId, revision })}
        >
          Done
        </button>
      ) : null}
    </div>
  );
}

function AttentionRow({
  binding,
  canManage,
  onGoalConfirm,
  onGoalRequestClose,
  onAttentionPause,
  onAttentionResume,
  onAttentionReassign,
}: {
  binding: WorkGraphWireBinding;
  canManage: boolean;
} & Pick<WorkGraphPanelProps,
  "onGoalConfirm" | "onGoalRequestClose" | "onAttentionPause" | "onAttentionResume" | "onAttentionReassign"
>) {
  const [reassignOpen, setReassignOpen] = React.useState(false);
  const [reassignIdentity, setReassignIdentity] = React.useState("");
  const bindingId = binding.binding_id || "";
  const revision = binding.machine_state?.revision;
  const statusLabel = workGraphBindingStatusLabel(binding);
  const targetLabel = workGraphBindingTargetLabel(binding);
  const isActive = statusLabel === "active";
  const isPaused = statusLabel.startsWith("paused");
  const live = isActive || isPaused;
  const input = { bindingId, revision };
  if (!bindingId) return null;
  return (
    <div className="workgraph__binding" data-testid={`workgraph-panel-binding:${bindingId}`}>
      <div className="workgraph__binding-line">
        <span className={`workgraph__mode is-${binding.mode || "pursue"}`}>{binding.mode || "pursue"}</span>
        <span className="workgraph__binding-status">{statusLabel}</span>
        {targetLabel ? <span className="workgraph__binding-target">{targetLabel}</span> : null}
        {binding.work_ref?.item_id ? (
          <span className="workgraph__chip" title="Bound work item">{binding.work_ref.item_id}</span>
        ) : null}
        <span className="workgraph__spacer" />
        {canManage && onAttentionPause && isActive ? (
          <button type="button" className="workgraph__action" onClick={() => onAttentionPause(input)}>Pause</button>
        ) : null}
        {canManage && onAttentionResume && isPaused ? (
          <button type="button" className="workgraph__action" onClick={() => onAttentionResume(input)}>Resume</button>
        ) : null}
        {canManage && onGoalConfirm && live ? (
          <button type="button" className="workgraph__action" onClick={() => onGoalConfirm(input)}>Confirm</button>
        ) : null}
        {canManage && onGoalRequestClose && live ? (
          <button type="button" className="workgraph__action" onClick={() => onGoalRequestClose(input)}>Request close</button>
        ) : null}
        {canManage && onAttentionReassign && live ? (
          <button
            type="button"
            className="workgraph__action"
            aria-expanded={reassignOpen}
            onClick={() => setReassignOpen((value) => !value)}
          >
            Reassign
          </button>
        ) : null}
      </div>
      {reassignOpen && canManage && onAttentionReassign ? (
        <div className="workgraph__reassign">
          <input
            placeholder="Target agent identity…"
            value={reassignIdentity}
            onChange={(event) => setReassignIdentity(event.target.value)}
            data-testid={`workgraph-panel-reassign-input:${bindingId}`}
          />
          <button
            type="button"
            className="workgraph__action"
            disabled={!reassignIdentity.trim()}
            data-testid={`workgraph-panel-reassign-submit:${bindingId}`}
            onClick={() => {
              onAttentionReassign({ ...input, identity: reassignIdentity.trim() });
              setReassignOpen(false);
              setReassignIdentity("");
            }}
          >
            Reassign to identity
          </button>
        </div>
      ) : null}
    </div>
  );
}

export function WorkGraphPanel({
  data,
  canManage,
  onRefresh,
  onClaim,
  onClose,
  onGoalConfirm,
  onGoalRequestClose,
  onAttentionPause,
  onAttentionResume,
  onAttentionReassign,
}: WorkGraphPanelProps): React.JSX.Element {
  const rows = React.useMemo(
    () => buildWorkGraphPanelTree(data.items, data.edges),
    [data.items, data.edges],
  );

  if (data.unavailable) {
    return (
      <div className="console-panel workgraph" data-testid="workgraph-panel">
        <div className="workgraph__empty">WorkGraph is not configured on this runtime.</div>
      </div>
    );
  }

  return (
    <div className="console-panel workgraph" data-testid="workgraph-panel">
      <div className="workgraph__head">
        <h3>WorkGraph</h3>
        {data.capturedAt ? (
          <span className="workgraph__captured">as of {data.capturedAt.slice(0, 19).replace("T", " ")}</span>
        ) : null}
        <span className="workgraph__spacer" />
        <button
          type="button"
          className="workgraph__action"
          onClick={onRefresh}
          data-testid="workgraph-panel-refresh"
        >
          Refresh
        </button>
      </div>
      {data.error ? <div className="workgraph__error" role="alert">{data.error}</div> : null}
      {data.denied ? (
        <div className="workgraph__empty" data-testid="workgraph-panel-denied">
          You do not have a grant to view WorkGraph state.
        </div>
      ) : (
        <>
          <div className="workgraph__section">
            <div className="workgraph__sec-label">Work items</div>
            {rows.length === 0 ? (
              <div className="workgraph__empty">No work items.</div>
            ) : (
              rows.map((row) => (
                <ItemRow
                  key={row.itemId}
                  row={row}
                  canManage={canManage}
                  onClaim={onClaim}
                  onClose={onClose}
                />
              ))
            )}
          </div>
          <div className="workgraph__section">
            <div className="workgraph__sec-label">Attention</div>
            {data.attention.length === 0 ? (
              <div className="workgraph__empty">No attention bindings.</div>
            ) : (
              data.attention.map((binding, index) => (
                <AttentionRow
                  key={binding.binding_id || `binding-${index}`}
                  binding={binding}
                  canManage={canManage}
                  onGoalConfirm={onGoalConfirm}
                  onGoalRequestClose={onGoalRequestClose}
                  onAttentionPause={onAttentionPause}
                  onAttentionResume={onAttentionResume}
                  onAttentionReassign={onAttentionReassign}
                />
              ))
            )}
          </div>
          <div className="workgraph__section">
            <div className="workgraph__sec-label">Recent events</div>
            {data.events.length === 0 ? (
              <div className="workgraph__empty">No events.</div>
            ) : (
              <div className="workgraph__events">
                {data.events.map((event, index) => (
                  <div className="workgraph__event" key={`${event.seq ?? index}`}>
                    {workGraphEventLine(event)}
                  </div>
                ))}
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}
