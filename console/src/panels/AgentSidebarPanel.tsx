import React from "react";
import type { ConsoleAgent } from "../types";

interface AgentSidebarPanelProps {
  title: string;
  agents: ConsoleAgent[];
  onSelectMember: (memberId: string) => void;
}

export function AgentSidebarPanel({
  title,
  agents,
  onSelectMember,
}: AgentSidebarPanelProps): React.JSX.Element {
  return (
    <section data-testid="agent-sidebar">
      <h2>{title}</h2>
      <ul data-testid="sidebar-list">
        {agents.map((agent) => (
          <li key={agent.member_id}>
            <button
              type="button"
              data-agent-id={agent.agent_id}
              onClick={() => onSelectMember(agent.member_id)}
            >
              {agent.label}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
