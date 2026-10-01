import React from "react";
import { ApprovalCard } from "../../../packages/console-components/src/conversation/approval-card";
import type { PendingApprovalSnapshot } from "../../../packages/console-core/src/pending-approvals";

interface GatingInboxPanelProps {
  pending: unknown[];
  audit: unknown[];
  onDecide: (pendingRef: string, decision: "approve" | "reject" | "escalate") => void;
  readOnly?: boolean;
  resource?: PendingApprovalSnapshot;
  onRefresh?: () => void;
  selectedPendingRef?: string;
}

type Tab = "pending" | "auto" | "audit" | "policies";

function getRisk(entry: Record<string, unknown>): "low" | "medium" | "high" {
  const tier = String(entry.risk_tier || entry.risk || "").toLowerCase();
  if (tier === "high" || tier === "crit" || tier === "critical") return "high";
  if (tier === "medium" || tier === "med" || tier === "warn") return "medium";
  return "low";
}

function formatWaited(entry: Record<string, unknown>): string {
  const waited = entry.waited_ms || entry.waited || entry.age_ms;
  if (typeof waited !== "number") return "—";
  const seconds = Math.floor(waited / 1000);
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes}m`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${minutes % 60}m`;
}

function payloadSummary(entry: Record<string, unknown>): string {
  const payload = entry.payload;
  if (typeof payload === "string") return payload;
  if (payload && typeof payload === "object") {
    try {
      const parts: string[] = [];
      for (const [k, v] of Object.entries(payload).slice(0, 3)) {
        parts.push(`${k}=${String(v).slice(0, 20)}`);
      }
      return parts.join(" ");
    } catch { return ""; }
  }
  return String(entry.summary || entry.reason || "");
}

export function GatingInboxPanel({
  pending,
  audit,
  onDecide,
  readOnly = false,
  resource,
  onRefresh,
  selectedPendingRef,
}: GatingInboxPanelProps): React.JSX.Element {
  const [tab, setTab] = React.useState<Tab>("pending");
  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const listRef = React.useRef<HTMLDivElement>(null);
  const pendingRequests = resource?.requests.filter((request) => request.status === "pending" && resource.decisions[request.pendingRef]?.phase !== "settled");
  const pendingLabel = !resource || resource.status === "ready" ? String(pendingRequests ? pendingRequests.length : pending.length) : "?";
  const selectedRequestAvailable = resource?.requests.some((request) => request.pendingRef === selectedPendingRef) === true;
  React.useEffect(() => { if (selectedPendingRef) { setSelectedId(selectedPendingRef); setTab("pending"); } }, [selectedPendingRef]);
  React.useEffect(() => {
    if (tab !== "pending" || !selectedPendingRef || !selectedRequestAvailable) return;
    const selected = Array.from(listRef.current?.querySelectorAll<HTMLElement>("[data-approval-id]") || [])
      .find((element) => element.dataset.approvalId === selectedPendingRef);
    selected?.scrollIntoView?.({ block: "nearest" });
    selected?.focus({ preventScroll: true });
  }, [selectedPendingRef, selectedRequestAvailable, tab]);

  const autoApproved = audit.filter((e) => {
    const r = e as Record<string, unknown>;
    return String(r.decision || "").toLowerCase() === "auto_approve" ||
           String(r.event_type || "").includes("auto");
  });

  const currentList: unknown[] =
    tab === "pending" ? pending :
    tab === "auto" ? autoApproved :
    audit;

  return (
    <div className="gating" data-testid="gating-panel">
      <div className="gating__head">
        <h2>Approvals</h2>
        <p>· {pendingLabel} pending · {autoApproved.length} auto-approved</p>
      </div>
      <div className="gating__tabs">
        <button
          className={`gating__tab ${tab === "pending" ? "is-active" : ""}`}
          onClick={() => setTab("pending")}
          data-testid="gating-tab:pending"
        >
          Pending <span className="n">{pendingLabel}</span>
        </button>
        <button
          className={`gating__tab ${tab === "auto" ? "is-active" : ""}`}
          onClick={() => setTab("auto")}
          data-testid="gating-tab:auto"
        >
          Auto <span className="n">{autoApproved.length}</span>
        </button>
        <button
          className={`gating__tab ${tab === "audit" ? "is-active" : ""}`}
          onClick={() => setTab("audit")}
          data-testid="gating-tab:audit"
        >
          Audit <span className="n">{audit.length}</span>
        </button>
        <button
          className={`gating__tab ${tab === "policies" ? "is-active" : ""}`}
          onClick={() => setTab("policies")}
          data-testid="gating-tab:policies"
        >
          Policies
        </button>
      </div>
      <div className="gating__list" ref={listRef}>
        {tab === "pending" && resource ? (
          <div>
            {resource.status !== "ready" ? <p role="status">{resource.status === "forbidden" ? "Approval access denied" : resource.status === "unsupported" ? "Approvals are not available for this connection" : resource.status === "loading" ? "Loading approvals" : resource.status === "stale" ? "Approvals may be out of date" : "Approvals unavailable"}</p> : null}
            {onRefresh ? <button type="button" onClick={onRefresh}>Refresh approvals</button> : null}
            {resource.status === "ready" && pendingRequests?.length === 0 ? <p>No pending approvals.</p> : null}
            {resource.requests.map((request) => <div key={request.pendingRef} tabIndex={-1} data-approval-id={request.pendingRef} data-selected={selectedId === request.pendingRef} className={selectedId === request.pendingRef ? "is-selected" : undefined}>
              <ApprovalCard request={request} resourceStatus={resource.status} decision={resource.decisions[request.pendingRef]} readOnly={readOnly || resource.readOnly} onDecide={onDecide} />
            </div>)}
          </div>
        ) : tab === "policies" ? (
          <div className="gating__empty" role="status">Policy details are not available in this console.</div>
        ) : (
          <>
          {currentList.length === 0 && (
          <div className="gating__empty">No {tab} items.</div>
          )}
          {currentList.map((entry, index) => {
          const r = entry as Record<string, unknown>;
          // Decisions name the opaque ref; the display ID is only a label.
          const pendingRef = typeof r.pending_ref === "string" && r.pending_ref ? r.pending_ref : undefined;
          const pid = String(pendingRef || r.audit_id || `item-${index}`);
          const label = String(r.pending_id || r.audit_id || `item-${index}`);
          const action = String(r.action_id || r.event_type || "unknown action");
          const agent = String(r.agent || r.identity || r.actor || "");
          const waited = formatWaited(r);
          const risk = getRisk(r);
          const payload = payloadSummary(r);

          const selected = selectedId === pid;
          const showActions = tab === "pending" && !readOnly && pendingRef !== undefined;

          return (
            <div
              className={`gitem ${selected ? "is-selected" : ""}`}
              data-risk={risk}
              data-testid={`gating-pending:${pid}`}
              key={pid}
              onClick={() => setSelectedId(pid)}
            >
              <span className="gitem__risk" />
              <span className="gitem__id">{label.slice(0, 8)}</span>
              <span>
                <div className="gitem__action">{action}</div>
                {payload && <div className="gitem__payload">{payload}</div>}
                {agent && <div className="gitem__agent">{agent}</div>}
              </span>
              {showActions ? (
                <span className="gitem__actions">
                  <button
                    className="approve"
                    data-testid={`gating-action:${pid}:approve`}
                    onClick={(e) => { e.stopPropagation(); onDecide(pid, "approve"); }}
                  >Approve</button>
                  <button
                    className="reject"
                    data-testid={`gating-action:${pid}:reject`}
                    onClick={(e) => { e.stopPropagation(); onDecide(pid, "reject"); }}
                  >Reject</button>
                  <button
                    data-testid={`gating-action:${pid}:escalate`}
                    onClick={(e) => { e.stopPropagation(); onDecide(pid, "escalate"); }}
                  >Escalate</button>
                </span>
              ) : (
                <span className="gitem__actions" />
              )}
              <span className="gitem__waited">waited<br />{waited}</span>
            </div>
          );
          })}
          </>
        )}
      </div>
    </div>
  );
}
