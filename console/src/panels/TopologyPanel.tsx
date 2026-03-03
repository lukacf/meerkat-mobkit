import React from "react";

interface TopologyPanelProps {
  title: string;
  nodeCount: number;
  nodes: string[];
}

export function TopologyPanel({ title, nodeCount, nodes }: TopologyPanelProps): React.JSX.Element {
  return (
    <section data-testid="topology-panel">
      <h2>{title}</h2>
      <p data-testid="topology-node-count">{`Node count: ${nodeCount}`}</p>
      <ul data-testid="topology-nodes">
        {nodes.map((moduleId) => (
          <li key={moduleId}>{moduleId}</li>
        ))}
      </ul>
    </section>
  );
}
