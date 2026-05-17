import React from "react";
import type { ConsoleAgent, ConsoleFrame, ConsoleTopologyNode } from "../types";
import { ForceDirected } from "./topology/ForceDirected";
import { Bullseye } from "./topology/Bullseye";
import { RoleTree } from "./topology/RoleTree";
import { DenseGraphMap } from "./topology/DenseGraphMap";
import { buildGraph, useTopologyActivity, colourForRole, roleIndexFor } from "./topology/data";

interface TopologyPanelProps {
  nodes: ConsoleTopologyNode[];
  agents: ConsoleAgent[];
  activity: ConsoleFrame[];
}

type View = "graph" | "force" | "bullseye" | "roles";
type LabelsMode = "auto" | "on" | "off";
type EdgeMode = "all" | "focus";
const VIEW_STORAGE = "mobkit-console-topology-view";
const LABELS_STORAGE = "mobkit-console-topology-labels";
const EDGE_STORAGE = "mobkit-console-topology-edges";
const VIEWS: Array<{ id: View; label: string; help: string }> = [
  { id: "graph", label: "Graph", help: "Dense canvas graph with every node in one view" },
  { id: "force", label: "Force", help: "Physics sim · communities + hubs emerge" },
  { id: "bullseye", label: "Bullseye", help: "Degree-ranked rings · hubs at centre" },
  { id: "roles", label: "Roles", help: "Flat mob · agents grouped by role" },
];
const LABEL_MODES: Array<{ id: LabelsMode; label: string; help: string }> = [
  { id: "auto", label: "Auto",  help: "Always-on for ≤20 agents · hover for denser graphs" },
  { id: "on",   label: "All",   help: "Force labels on regardless of density" },
  { id: "off",  label: "Hover", help: "Hidden until hovered or focused" },
];
const EDGE_MODES: Array<{ id: EdgeMode; label: string; help: string }> = [
  { id: "all", label: "All", help: "Draw all graph edges persistently" },
  { id: "focus", label: "Focus", help: "Show only hovered-agent edges" },
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
      if (stored === "summary") return "graph";
      if (stored === "graph" || stored === "force" || stored === "bullseye" || stored === "roles") return stored;
    } catch { /* ignore */ }
    return "graph";
  });
  const pickView = (next: View) => {
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

  const [edgeMode, setEdgeMode] = React.useState<EdgeMode>(() => {
    try {
      const stored = localStorage.getItem(EDGE_STORAGE);
      if (stored === "all" || stored === "focus") return stored;
    } catch { /* ignore */ }
    return "all";
  });
  const pickEdgeMode = (next: EdgeMode) => {
    setEdgeMode(next);
    try { localStorage.setItem(EDGE_STORAGE, next); } catch { /* ignore */ }
  };

  const graph = React.useMemo(() => buildGraph(nodes, agents), [nodes, agents]);
  const live = useTopologyActivity(activity, graph, { life: 1500 });
  const roleIndex = React.useMemo(() => roleIndexFor(graph.roles), [graph.roles]);
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
        {view === "graph" && (
          <div className="topo__viewbar topo__viewbar--labels" role="group" aria-label="Edges">
            <span className="topo__viewbar-tag">Edges</span>
            {EDGE_MODES.map((m) => (
              <button
                key={m.id}
                type="button"
                className={`topo__viewbtn ${edgeMode === m.id ? "is-active" : ""}`}
                onClick={() => pickEdgeMode(m.id)}
                title={m.help}
                data-testid={`topology-edges:${m.id}`}
              >
                {m.label}
              </button>
            ))}
          </div>
        )}
        {view !== "roles" && view !== "graph" && (
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
        {view === "graph" && <DenseGraphMap graph={graph} edgeMode={edgeMode} />}
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
      {view !== "roles" && view !== "graph" && graph.roles.length > 0 && (
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
