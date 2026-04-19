import React from "react";

interface GatesPanelProps {
  audit: unknown[];
}

interface GatePolicy {
  id: string;
  action: string;
  scope: string;
  state: "active" | "paused";
  thresh: string;
  approvers: string[];
  sla: string;
  approved: number;
  rejected: number;
  escalated: number;
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
    sla: "— · p95 n/a",
    approved: s.approved,
    rejected: s.rejected,
    escalated: s.escalated,
  }));
}

export function GatesPanel({ audit }: GatesPanelProps): React.JSX.Element {
  const policies = React.useMemo(() => derivePolicies(audit), [audit]);
  const [sel, setSel] = React.useState(policies[0]?.id || "");
  const active = policies.find((p) => p.id === sel) || policies[0];

  return (
    <div className="view gates" data-testid="gates-panel">
      <div className="view__head">
        <h2>Gates</h2>
        <span className="view__sub">
          {policies.length} policies · {audit.length} decisions (recent)
        </span>
        <span className="view__spacer" />
      </div>
      <div className="gates__body">
        <div className="gates__list">
          {policies.map((g) => (
            <div
              key={g.id}
              className={`gate ${g.id === sel ? "is-selected" : ""}`}
              data-state={g.state}
              onClick={() => setSel(g.id)}
              data-testid={`gate-policy:${g.id}`}
            >
              <div className="gate__head">
                <span className="gate__action mono">{g.action}</span>
                <span className={`gate__state gate__state--${g.state}`}>{g.state}</span>
              </div>
              <div className="gate__scope">scope: {g.scope}</div>
              <div className="gate__thresh">{g.thresh}</div>
              <div className="gate__stats">
                <span><b>{g.approved}</b><span className="dim"> approved</span></span>
                <span><b>{g.rejected}</b><span className="dim"> rejected</span></span>
                <span><b>{g.escalated}</b><span className="dim"> escalated</span></span>
              </div>
            </div>
          ))}
          {policies.length === 0 && (
            <div style={{ padding: 24, color: "var(--ink-dim)", fontFamily: "var(--mono)", fontSize: 12 }}>
              No gate policies inferred from recent audit.
            </div>
          )}
        </div>
        <aside className="gates__detail">
          {active && (
            <>
              <div className="gd__title">{active.action}</div>
              <div className="gd__scope dim">scope: {active.scope}</div>
              <div className="gd__section">
                <div className="gd__label">Policy</div>
                <div className="gd__body">{active.thresh}</div>
              </div>
              <div className="gd__section">
                <div className="gd__label">Approvers</div>
                <div className="gd__approvers">
                  {active.approvers.length === 0 && <span className="chip">none recorded</span>}
                  {active.approvers.map((a) => (
                    <span key={a} className="chip">{a}</span>
                  ))}
                </div>
              </div>
              <div className="gd__section">
                <div className="gd__label">SLA</div>
                <div className="gd__body mono">{active.sla}</div>
              </div>
              <div className="gd__chart">
                <div className="gd__chart-label">Decisions (recent audit)</div>
                <div className="gd__bar">
                  <span className="gd__bar-ok" style={{ flex: active.approved || 0.001 }} />
                  <span className="gd__bar-no" style={{ flex: active.rejected || 0.001 }} />
                  <span className="gd__bar-up" style={{ flex: active.escalated || 0.001 }} />
                </div>
                <div className="gd__legend">
                  <span><i className="dot ok" /> {active.approved} approved</span>
                  <span><i className="dot no" /> {active.rejected} rejected</span>
                  <span><i className="dot up" /> {active.escalated} escalated</span>
                </div>
              </div>
            </>
          )}
        </aside>
      </div>
    </div>
  );
}
