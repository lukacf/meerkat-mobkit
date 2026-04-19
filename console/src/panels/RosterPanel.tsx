import React from "react";
import type { ConsoleAgent } from "../types";

interface RosterPanelProps {
  agents: ConsoleAgent[];
  onSelect: (agent: ConsoleAgent) => void;
  onInspect: (agent: ConsoleAgent) => void;
  onLifecycle: (identity: string, method: "mobkit/retire" | "mobkit/respawn" | "mobkit/reset") => void;
}

const ROLE_BUCKETS = ["all", "personal", "coordinator", "domain", "internal"] as const;
type Role = typeof ROLE_BUCKETS[number];

function roleOf(a: ConsoleAgent): Role {
  const p = (a.profile || a.kind || "").toLowerCase();
  const g = (a.group || "").toLowerCase();
  if (p.includes("personal") || g.includes("personal")) return "personal";
  if (p.includes("coord") || p.includes("triage") || p.includes("router")) return "coordinator";
  if (p.includes("monitor") || p.includes("scribe") || p.includes("gate")) return "internal";
  return "domain";
}

function stateLabel(state?: string): string {
  return (state || "unknown").toLowerCase();
}

export function RosterPanel({ agents, onSelect, onInspect, onLifecycle }: RosterPanelProps): React.JSX.Element {
  const [q, setQ] = React.useState("");
  const [role, setRole] = React.useState<Role>("all");
  const [sel, setSel] = React.useState<string>(agents[0]?.member_id || "");

  const rows = React.useMemo(() => {
    return agents.filter((a) => {
      if (role !== "all" && roleOf(a) !== role) return false;
      if (!q) return true;
      const hay = `${a.label} ${a.member_id} ${a.identity || ""} ${a.profile || ""} ${a.kind || ""}`.toLowerCase();
      return hay.includes(q.toLowerCase());
    });
  }, [agents, q, role]);

  const active = rows.find((r) => r.member_id === sel) || rows[0];
  const activeIdentity = active?.identity || active?.member_id || "";

  return (
    <div className="view roster" data-testid="roster-panel">
      <div className="view__head">
        <h2>Roster</h2>
        <span className="view__sub">{rows.length} of {agents.length} agents</span>
        <span className="view__spacer" />
        <input
          className="view__search"
          placeholder="Filter agents, profiles, ids…"
          value={q}
          onChange={(e) => setQ(e.target.value)}
        />
        <div className="view__segs">
          {ROLE_BUCKETS.map((r) => (
            <button key={r} className={role === r ? "is-active" : ""} onClick={() => setRole(r)}>{r}</button>
          ))}
        </div>
      </div>
      <div className="roster__body">
        <div className="roster__table">
          <div className="roster__row roster__row--head">
            <span>Name</span><span>Role</span><span>State</span><span>Profile</span><span>Gen</span><span>Chk</span><span>Lease</span>
          </div>
          {rows.map((r) => {
            const isSel = active && r.member_id === active.member_id;
            return (
              <div
                key={r.member_id}
                className={`roster__row ${isSel ? "is-selected" : ""}`}
                data-state={stateLabel(r.state)}
                onClick={() => { setSel(r.member_id); onSelect(r); }}
                data-testid={`roster-row:${r.member_id}`}
              >
                <span className="roster__name">
                  <span className="roster__dot" />
                  <span>
                    <div>{r.label}</div>
                    <div className="roster__id">{r.identity || r.member_id}</div>
                  </span>
                </span>
                <span>{roleOf(r)}</span>
                <span className="roster__state">{stateLabel(r.state)}</span>
                <span className="mono dim">{r.profile || "—"}</span>
                <span className="mono">{r.generation ?? "—"}</span>
                <span className="mono">{r.checkpoint_version ?? "—"}</span>
                <span className="mono dim">{r.lease_healthy === false ? "unhealthy" : "ok"}</span>
              </div>
            );
          })}
        </div>
        <aside className="roster__detail">
          {active && (
            <>
              <div className="rd__head">
                <div className="rd__title">{active.label}</div>
                <div className="rd__id">{active.identity || active.member_id}</div>
                <div className="rd__tags">
                  {[active.profile, active.kind, roleOf(active)].filter(Boolean).map((t) => (
                    <span key={String(t)} className="chip">{String(t)}</span>
                  ))}
                </div>
              </div>
              <dl className="rd__grid">
                <dt>Profile</dt><dd>{active.profile || "—"}</dd>
                <dt>Kind</dt><dd>{active.kind || "—"}</dd>
                <dt>Role</dt><dd>{roleOf(active)}</dd>
                <dt>State</dt><dd><span className="roster__state">{stateLabel(active.state)}</span></dd>
                <dt>Member</dt><dd className="mono">{active.member_id}</dd>
                <dt>Identity</dt><dd className="mono">{active.identity || "—"}</dd>
                <dt>Session</dt><dd className="mono">{active.session_id || "—"}</dd>
                <dt>Generation</dt><dd className="mono">{active.generation ?? "—"}</dd>
                <dt>Checkpoint</dt><dd className="mono">{active.checkpoint_version ?? "—"}</dd>
                <dt>Lease</dt><dd className="mono">{active.lease_healthy === false ? "unhealthy" : "ok"}</dd>
                <dt>Wired</dt><dd className="mono">{(active.wired_to || []).length} peers</dd>
              </dl>
              <div className="rd__actions">
                <button onClick={() => onInspect(active)}>Inspect</button>
                <button
                  disabled={!active.affordances?.can_respawn}
                  onClick={() => onLifecycle(activeIdentity, "mobkit/respawn")}
                >Respawn</button>
                <button onClick={() => onLifecycle(activeIdentity, "mobkit/reset")}>Reset</button>
                <button
                  className="danger"
                  disabled={!active.affordances?.can_retire}
                  onClick={() => onLifecycle(activeIdentity, "mobkit/retire")}
                >Retire</button>
              </div>
            </>
          )}
        </aside>
      </div>
    </div>
  );
}
