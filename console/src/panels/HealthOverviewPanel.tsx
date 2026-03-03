import React from "react";

interface HealthOverviewPanelProps {
  title: string;
  running: boolean | null;
  loadedModuleCount: number;
  loadedModules: string[];
}

export function HealthOverviewPanel({
  title,
  running,
  loadedModuleCount,
  loadedModules,
}: HealthOverviewPanelProps): React.JSX.Element {
  return (
    <section data-testid="health-overview">
      <h2>{title}</h2>
      <p data-testid="health-running">{`Running: ${running === null ? "unknown" : String(running)}`}</p>
      <p data-testid="health-loaded-module-count">{`Loaded module count: ${loadedModuleCount}`}</p>
      <ul data-testid="health-loaded-modules">
        {loadedModules.map((moduleId) => (
          <li key={moduleId}>{moduleId}</li>
        ))}
      </ul>
    </section>
  );
}
