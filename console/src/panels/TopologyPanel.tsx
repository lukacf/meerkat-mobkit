import React from "react";
import type { ConsoleAgent, ConsoleFrame, ConsoleTopologyNode } from "../types";
import { ForceDirected } from "./topology/ForceDirected";
import { Bullseye } from "./topology/Bullseye";
import { RoleTree } from "./topology/RoleTree";
import { LargeGraphSummary } from "./topology/LargeGraphSummary";
import { buildGraph, useTopologyActivity, colourForRole, roleIndexFor } from "./topology/data";

interface TopologyPanelProps {
  nodes: ConsoleTopologyNode[];
  agents: ConsoleAgent[];
  activity: ConsoleFrame[];
}

type View = "summary" | "force" | "bullseye" | "roles";
type LabelsMode = "auto" | "on" | "off";
const VIEW_STORAGE = "mobkit-console-topology-view";
const LABELS_STORAGE = "mobkit-console-topology-labels";
const VIEWS: Array<{ id: View; label: string; help: string }> = [
  { id: "summary", label: "Summary", help: "Aggregate scale, groups, and selected ego network" },
  { id: "force", label: "Force", help: "Physics sim · communities + hubs emerge" },
  { id: "bullseye", label: "Bullseye", help: "Degree-ranked rings · hubs at centre" },
  { id: "roles", label: "Roles", help: "Flat mob · agents grouped by role" },
];
const LABEL_MODES: Array<{ id: LabelsMode; label: string; help: string }> = [
  { id: "auto", label: "Auto",  help: "Always-on for ≤20 agents · hover for denser graphs" },
  { id: "on",   label: "All",   help: "Force labels on regardless of density" },
  { id: "off",  label: "Hover", help: "Hidden until hovered or focused" },
];

const W = 980;
const H = 580;

export function TopologyPanel({
  nodes,
  agents,
  activity,
}: TopologyPanelProps): React.JSX.Element {
  const [view, setView] = React.useState<View>(() => {
    try {
      const stored = localStorage.getItem(VIEW_STORAGE);
      if (stored === "summary" || stored === "force" || stored === "bullseye" || stored === "roles") return stored;
    } catch { /* ignore */ }
    return "summary";
  });
  const [userPickedView, setUserPickedView] = React.useState(false);
  const pickView = (next: View) => {
    setUserPickedView(true);
    setView(next);
    try { localStorage.setItem(VIEW_STORAGE, next); } catch { /* ignore */ }
  };

  const [labelsMode, setLabelsMode] = React.useState<LabelsMode>(() => {
    try {
      const stored = localStorage.getItem(LABELS_STORAGE);
      if (stored === "auto" || stored === "on" || stored === "off") return stored;
    } catch { /* ignore */ }
    return "auto";
  });
  const pickLabelsMode = (next: LabelsMode) => {
    setLabelsMode(next);
    try { localStorage.setItem(LABELS_STORAGE, next); } catch { /* ignore */ }
  };

  const graph = React.useMemo(() => buildGraph(nodes, agents), [nodes, agents]);
  const live = useTopologyActivity(activity, graph, { life: 1500 });
  const roleIndex = React.useMemo(() => roleIndexFor(graph.roles), [graph.roles]);
  const denseGraph = graph.agents.length >= 150 || graph.edges.length >= 3000;

  React.useEffect(() => {
    if (!denseGraph || userPickedView || view === "summary") return;
    setView("summary");
    try { localStorage.setItem(VIEW_STORAGE, "summary"); } catch { /* ignore */ }
  }, [denseGraph, userPickedView, view]);

  const liveCount = Object.keys(live.active).length;
  const busyCount = Object.values(live.busy).filter(Boolean).length;

  return (
    <div className="topo" data-testid="topology-panel">
      <div className="topo__head">
        <h2>Topology</h2>
        <span className="topo__head-meta">
          {graph.agents.length} agents · {graph.edges.length} edges
          {busyCount > 0 ? ` · ${busyCount} working` : ""}
          {liveCount > 0 && busyCount === 0 ? ` · ${liveCount} live` : ""}
        </span>
        {view !== "roles" && view !== "summary" && (
          <div className="topo__viewbar topo__viewbar--labels" role="group" aria-label="Labels">
            <span className="topo__viewbar-tag">Labels</span>
            {LABEL_MODES.map((m) => (
              <button
                key={m.id}
                type="button"
                className={`topo__viewbtn ${labelsMode === m.id ? "is-active" : ""}`}
                onClick={() => pickLabelsMode(m.id)}
                title={m.help}
                data-testid={`topology-labels:${m.id}`}
              >
                {m.label}
              </button>
            ))}
          </div>
        )}
        <div className="topo__viewbar">
          {VIEWS.map((v) => (
            <button
              key={v.id}
              type="button"
              className={`topo__viewbtn ${view === v.id ? "is-active" : ""}`}
              onClick={() => pickView(v.id)}
              title={v.help}
              data-testid={`topology-view:${v.id}`}
            >
              {v.label}
            </button>
          ))}
        </div>
      </div>
      <div className="topo__body">
        {view === "summary" && (
          <LargeGraphSummary
            graph={graph}
            live={live}
          />
        )}
        {view === "force" && (
          <ForceDirected
            nodes={nodes}
            agents={agents}
            activity={activity}
            width={W}
            height={H}
            labelsMode={labelsMode}
          />
        )}
        {view === "bullseye" && (
          <Bullseye
            nodes={nodes}
            agents={agents}
            activity={activity}
            width={W}
            height={H}
            labelsMode={labelsMode}
          />
        )}
        {view === "roles" && (
          <RoleTree
            nodes={nodes}
            agents={agents}
            activity={activity}
          />
        )}
      </div>
      {view !== "roles" && view !== "summary" && graph.roles.length > 0 && (
        <div className="topo__legend">
          {graph.roles.map((role) => {
            const count = graph.agents.filter((a) => a.role === role).length;
            return (
              <div key={role} className="topo__legend-item">
                <span
                  className="topo__legend-dot"
                  style={{ background: colourForRole(role, roleIndex) }}
                />
                {role}
                <span className="topo__legend-count">{count}</span>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
