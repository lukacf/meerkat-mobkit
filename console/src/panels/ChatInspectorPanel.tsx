import React from "react";
import type { ConsoleAgent, ConsoleFrame } from "../types";

interface ChatInspectorPanelProps {
  title: string;
  agents: ConsoleAgent[];
  selectedMemberId: string;
  onSelectedMemberIdChange: (value: string) => void;
  message: string;
  onMessageChange: (value: string) => void;
  onSubmit: (event: React.FormEvent<HTMLFormElement>) => void;
  frames: ConsoleFrame[];
}

export function ChatInspectorPanel({
  title,
  agents,
  selectedMemberId,
  onSelectedMemberIdChange,
  message,
  onMessageChange,
  onSubmit,
  frames,
}: ChatInspectorPanelProps): React.JSX.Element {
  return (
    <section data-testid="chat-inspector">
      <h2>{title}</h2>
      <form data-testid="chat-form" onSubmit={onSubmit}>
        <label>
          Member
          <select
            name="member"
            value={selectedMemberId}
            onChange={(event) => onSelectedMemberIdChange(event.target.value)}
          >
            {agents.map((agent) => (
              <option key={agent.member_id} value={agent.member_id}>
                {agent.member_id}
              </option>
            ))}
          </select>
        </label>
        <label>
          Message
          <textarea
            name="message"
            value={message}
            onChange={(event) => onMessageChange(event.target.value)}
          />
        </label>
        <button type="submit">Send</button>
      </form>
      <ul data-testid="chat-events">
        {frames.map((frame, index) => (
          <li key={`${frame.id || frame.event || "frame"}:${index}`}>
            {`${frame.event || "message"} ${frame.id || ""}`.trim()}
          </li>
        ))}
      </ul>
    </section>
  );
}
