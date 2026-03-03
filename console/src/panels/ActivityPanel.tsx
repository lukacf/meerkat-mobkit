import React from "react";
import type { ConsoleFrame } from "../types";

interface ActivityPanelProps {
  title: string;
  frames: ConsoleFrame[];
}

export function ActivityPanel({ title, frames }: ActivityPanelProps): React.JSX.Element {
  return (
    <section data-testid="activity-panel">
      <h2>{title}</h2>
      <ul data-testid="activity-feed">
        {frames.map((frame, index) => (
          <li key={`${frame.id || frame.event || "event"}:${index}`}>
            {`${frame.event || "message"} ${frame.id || ""}`.trim()}
          </li>
        ))}
      </ul>
    </section>
  );
}
