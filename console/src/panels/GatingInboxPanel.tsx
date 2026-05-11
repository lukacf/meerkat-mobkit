import React from "react";

interface GatingInboxPanelProps {
  pending: unknown[];
  audit: unknown[];
  onDecide: (pendingId: string, decision: "approve" | "reject" | "escalate") => void;
}

type Tab = "pending" | "auto" | "audit" | "policies";

interface GatePolicy {
  id: string;
  action: string;
  scope: string;
  state: "active" | "paused";
  thresh: string;
  approvers: string[];
  approved: number;
  rejected: number;
  escalated: number;
}

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

function derivePolicies(audit: unknown[]): GatePolicy[] {
  const byAction = new Map<string, { approved: number; rejected: number; escalated: number; approvers: Set<string> }>();
  for (const entry of audit) {
    const r = entry as Record<string, unknown>;
    const action = String(r.action_id || r.event_type || "unknown");
    const decision = String(r.decision || "").toLowerCase();
    const approver = String(r.approver_id || r.actor || "");
    const cur = byAction.get(action) || { approved: 0, rejected: 0, escalated: 0, approvers: new Set<string>() };
    if (decision === "approve" || decision === "auto_approve") cur.approved++;
    else if (decision === "reject") cur.rejected++;
    else if (decision === "escalate") cur.escalated++;
    if (approver) cur.approvers.add(approver);
    byAction.set(action, cur);
  }
  return Array.from(byAction.entries()).map(([action, s], i) => ({
    id: `pol-${i + 1}`,
    action,
    scope: "*",
    state: s.approved + s.rejected > 0 ? "active" : "paused",
    thresh: s.rejected > s.approved ? "High rejection rate" : "Auto on low risk",
    approvers: Array.from(s.approvers),
    approved: s.approved,
    rejected: s.rejected,
    escalated: s.escalated,
  }));
}

export function GatingInboxPanel({ pending, audit, onDecide }: GatingInboxPanelProps): React.JSX.Element {
  const [tab, setTab] = React.useState<Tab>("pending");
  const [selectedId, setSelectedId] = React.useState<string | null>(null);
  const policies = React.useMemo(() => derivePolicies(audit), [audit]);

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
        <p>· {pending.length} pending · {autoApproved.length} auto-approved · {policies.length} policies</p>
      </div>
      <div className="gating__tabs">
        <button
          className={`gating__tab ${tab === "pending" ? "is-active" : ""}`}
          onClick={() => setTab("pending")}
          data-testid="gating-tab:pending"
        >
          Pending <span className="n">{pending.length}</span>
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
          Policies <span className="n">{policies.length}</span>
        </button>
      </div>
      <div className="gating__list">
        {tab === "policies" ? (
          <div className="gating__policies">
            {policies.length === 0 && (
              <div className="gating__empty">No gate policies inferred from recent audit.</div>
            )}
            {policies.map((policy) => (
              <div className="gpolicy" data-state={policy.state} key={policy.id} data-testid={`gating-policy:${policy.id}`}>
                <div className="gpolicy__head">
                  <span className="gpolicy__action">{policy.action}</span>
                  <span className={`gpolicy__state gpolicy__state--${policy.state}`}>{policy.state}</span>
                </div>
                <div className="gpolicy__meta">scope: {policy.scope}</div>
                <div className="gpolicy__rule">{policy.thresh}</div>
                <div className="gpolicy__stats">
                  <span><b>{policy.approved}</b> approved</span>
                  <span><b>{policy.rejected}</b> rejected</span>
                  <span><b>{policy.escalated}</b> escalated</span>
                </div>
                <div className="gpolicy__approvers">
                  {policy.approvers.length === 0 ? (
                    <span className="chip">no approvers recorded</span>
                  ) : policy.approvers.map((approver) => (
                    <span className="chip" key={approver}>{approver}</span>
                  ))}
                </div>
              </div>
            ))}
          </div>
        ) : (
          <>
          {currentList.length === 0 && (
          <div className="gating__empty">No {tab} items.</div>
          )}
          {currentList.map((entry, index) => {
          const r = entry as Record<string, unknown>;
          const pid = String(r.pending_id || r.audit_id || `item-${index}`);
          const action = String(r.action_id || r.event_type || "unknown action");
          const agent = String(r.agent || r.identity || r.actor || "");
          const waited = formatWaited(r);
          const risk = getRisk(r);
          const payload = payloadSummary(r);

          const selected = selectedId === pid;
          const showActions = tab === "pending";

          return (
            <div
              className={`gitem ${selected ? "is-selected" : ""}`}
              data-risk={risk}
              data-testid={`gating-pending:${pid}`}
              key={pid}
              onClick={() => setSelectedId(pid)}
            >
              <span className="gitem__risk" />
              <span className="gitem__id">{pid.slice(0, 8)}</span>
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
