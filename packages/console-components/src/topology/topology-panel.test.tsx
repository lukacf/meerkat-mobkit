import { fireEvent, render, screen } from "@testing-library/react";

import { TopologyPanel } from "./topology-panel";

describe("TopologyPanel", () => {
  test("keeps role nodes passive instead of treating topology clicks as navigation", () => {
    window.localStorage.setItem("mobkit-console-topology-view", "roles");

    render(
      <TopologyPanel
        activity={[]}
        agents={[{
          agent_id: "project:sora2:release-evidence",
          member_id: "project:sora2:release-evidence",
          identity: "project:sora2:release-evidence",
          label: "Sora2 Release Steward",
          kind: "agent",
          role: "release",
          state: "ready",
          group: "Project agents",
          wired_to: [],
        }]}
        nodes={[]}
      />,
    );

    const node = screen.getByTestId("topology-node:project:sora2:release-evidence");
    expect(node.tagName).toBe("DIV");

    fireEvent.click(node);

    expect(screen.queryByTestId("topology-selection")).not.toBeInTheDocument();
  });
});
