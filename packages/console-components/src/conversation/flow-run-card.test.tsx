import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";

import {
  groupConversationTimelineEntries,
  type ConversationFlowRunEntry,
  type ConversationTimelineEntry,
} from "@console-core";

import { FlowRunCard } from "./flow-run-card";

function memberTranscript() {
  const entries: ConversationTimelineEntry[] = [
    {
      id: "builder-message",
      kind: "message",
      variant: "plain",
      identity: { id: "builder", label: "Builder", role: "other" },
      text: "Implemented the shared flow-run presentation.",
    },
  ];
  return {
    conversationId: "builder-transcript",
    entries,
    groups: groupConversationTimelineEntries(entries),
    turnDiff: null,
    emptyState: null,
  };
}

function entryFixture(overrides: Partial<ConversationFlowRunEntry> = {}): ConversationFlowRunEntry {
  return {
    id: "flow-run:release-crew",
    kind: "flow_run",
    identity: { id: "coordinator", label: "Coordinator", role: "assistant" },
    helperId: "helper-1",
    flowName: "Release crew",
    status: "running",
    rows: [
      {
        memberKey: "builder",
        label: "Builder",
        caption: "Implementing the shared component",
        status: "running",
        subView: memberTranscript(),
      },
    ],
    ...overrides,
  };
}

describe("FlowRunCard", () => {
  test("renders stopped state and non-expandable members as content with a unique message action", () => {
    const onMessageMember = vi.fn();
    const { container } = render(
      <FlowRunCard
        entry={entryFixture({
          status: "stopped",
          rows: [
            {
              memberKey: "reviewer",
              label: "Reviewer",
              caption: "Stopped by the operator",
              status: "stopped",
            },
          ],
        })}
        onMessageMember={onMessageMember}
      />,
    );

    expect(container.querySelector("[data-flow-run-card]")?.getAttribute("data-status"))
      .toBe("stopped");
    expect(screen.getAllByText("Stopped")).toHaveLength(2);
    expect(container.querySelector(".cc-flow-run__member-row")?.tagName).toBe("DIV");
    expect(screen.queryByRole("button", { name: /reviewer.*stopped/i })).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Message Reviewer" }));
    expect(onMessageMember).toHaveBeenCalledWith("reviewer");
  });

  test("distinguishes queued and cancelling agents from idle or completed work", () => {
    const { container } = render(
      <FlowRunCard
        entry={entryFixture({
          status: "cancelling",
          rows: [
            { memberKey: "builder", label: "Builder", caption: "Waiting for a safe stop", status: "cancelling" },
            { memberKey: "reviewer", label: "Reviewer", caption: "Waiting to start", status: "queued" },
            { memberKey: "observer", label: "Observer", caption: "Ready for work", status: "idle" },
          ],
        })}
      />,
    );

    expect(container.querySelector("[data-flow-run-card]")?.getAttribute("data-status"))
      .toBe("cancelling");
    expect(screen.getAllByText("Stopping")).toHaveLength(2);
    expect(screen.getByText("Queued", { selector: ".cc-flow-run__member-status" })).toBeTruthy();
    expect(screen.getByText("Idle", { selector: ".cc-flow-run__member-status" })).toBeTruthy();
  });

  test("keeps status in an expandable row's accessible name and labels its bounded transcript region", () => {
    const { container } = render(<FlowRunCard entry={entryFixture()} />);
    const rowButton = screen.getByRole("button", { name: /builder.*working/i });
    const detailId = rowButton.getAttribute("aria-controls");

    expect(rowButton.getAttribute("aria-expanded")).toBe("false");
    expect(screen.getByText("Working", { selector: ".cc-flow-run__member-status" }).textContent)
      .toBe("Working");

    fireEvent.click(rowButton);

    expect(rowButton.getAttribute("aria-expanded")).toBe("true");
    const region = screen.getByRole("region", { name: "Builder transcript" });
    expect(region.id).toBe(detailId);
    expect(region.getAttribute("tabindex")).toBe("0");
    expect(container.querySelector(".cc-flow-run__member-detail")?.textContent)
      .toContain("Implemented the shared flow-run presentation.");
  });
});
