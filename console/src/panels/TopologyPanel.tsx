import React from "react";
import type { ConsoleAgent, ConsoleFrame, ConsoleTopologyNode } from "../types";
import { DenseGraphMap } from "./topology/DenseGraphMap";
import { buildGraph } from "./topology/data";

interface TopologyPanelProps {
  nodes: ConsoleTopologyNode[];
  agents: ConsoleAgent[];
  activity: ConsoleFrame[];
}

export function TopologyPanel({
  nodes,
  agents,
  activity: _activity,
}: TopologyPanelProps): React.JSX.Element {
  const graph = React.useMemo(() => buildGraph(nodes, agents), [nodes, agents]);

  return (
    <div className="topo" data-testid="topology-panel">
      <div className="topo__head">
        <h2>Topology</h2>
        <span className="topo__head-meta">
          {graph.agents.length} agents · {graph.edges.length} edges
        </span>
      </div>
      <div className="topo__body">
        <DenseGraphMap graph={graph} />
      </div>
    </div>
  );
}
