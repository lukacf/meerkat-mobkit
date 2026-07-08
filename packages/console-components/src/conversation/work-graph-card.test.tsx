import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";

import type { ConversationWorkGraphEntry } from "@console-core";

import { WorkGraphCard } from "./work-graph-card";

function entryFixture(overrides: Partial<ConversationWorkGraphEntry> = {}): ConversationWorkGraphEntry {
  return {
    kind: "workgraph",
    id: "workgraph:goal-1",
    identity: { id: "planner", label: "Planner", role: "assistant" },
    rootId: "goal-1",
    title: "Release 0.7.30",
    objective: "Ship WorkGraph end to end",
    status: "active",
    progress: { completed: 1, total: 3 },
    items: [
      {
        itemId: "goal-1",
        title: "Release 0.7.30",
        status: "in_progress",
        revision: 4,
        depth: 0,
        description: "Ship WorkGraph end to end",
        updatedAt: "2026-07-08T09:00:00Z",
      },
      {
        itemId: "child-1",
        title: "Console card",
        status: "completed",
        revision: 2,
        depth: 1,
        parentId: "goal-1",
        ownerLabel: "Planner",
      },
      {
        itemId: "child-2",
        title: "SDK parity",
        status: "open",
        revision: 1,
        depth: 1,
        parentId: "goal-1",
        priority: "high",
      },
    ],
    attention: [
      {
        bindingId: "attention-1",
        mode: "pursue",
        statusLabel: "active",
        targetLabel: "sess-42",
        revision: 7,
      },
    ],
    recentEvents: ["item claimed · 09:15"],
    ...overrides,
  };
}

describe("WorkGraphCard", () => {
  test("renders the goal header, progress, status badge, item tree, and attention modes", () => {
    render(<WorkGraphCard entry={entryFixture()} />);

    const card = screen.getByTestId("workgraph-card:goal-1");
    expect(card).toHaveAttribute("data-status", "active");
    expect(card).toHaveAttribute("data-root-id", "goal-1");
    expect(screen.getByText("Release 0.7.30")).toBeInTheDocument();
    expect(screen.getByText("Ship WorkGraph end to end")).toBeInTheDocument();
    expect(screen.getByRole("progressbar")).toHaveAttribute("aria-valuenow", "1");
    expect(screen.getByText("1/3")).toBeInTheDocument();
    expect(screen.getByText("Console card")).toBeInTheDocument();
    expect(screen.getByText("SDK parity")).toBeInTheDocument();
    expect(screen.getByText("pursue")).toBeInTheDocument();
    expect(screen.getByText("high")).toBeInTheDocument();
  });

  test("renders terminal states without a live pulse", () => {
    const { container } = render(
      <WorkGraphCard
        entry={entryFixture({
          status: "completed",
          progress: { completed: 3, total: 3 },
        })}
      />,
    );
    expect(screen.getByTestId("workgraph-card:goal-1")).toHaveAttribute("data-status", "completed");
    expect(container.querySelector(".cc-work-graph__pulse")).toBeNull();
    expect(screen.getByText("Done")).toBeInTheDocument();
  });

  test("renders blocked state with blocked markers", () => {
    render(
      <WorkGraphCard
        entry={entryFixture({
          status: "blocked",
          items: [
            {
              itemId: "goal-1",
              title: "Release 0.7.30",
              status: "blocked",
              revision: 4,
              depth: 0,
              blocked: true,
            },
          ],
          attention: [],
        })}
      />,
    );
    expect(screen.getByTestId("workgraph-card:goal-1")).toHaveAttribute("data-status", "blocked");
    expect(screen.getByText("blocked")).toBeInTheDocument();
  });

  test("expands a row to reveal detail and collapses the whole card from the header toggle", () => {
    render(<WorkGraphCard entry={entryFixture()} />);

    fireEvent.click(screen.getByTestId("workgraph-item:goal-1"));
    expect(screen.getByText(/rev 4/)).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("workgraph-card:goal-1:toggle"));
    expect(screen.queryByText("Console card")).toBeNull();
  });

  test("renders no operator buttons without callbacks (undefined-handler convention)", () => {
    render(<WorkGraphCard entry={entryFixture()} />);
    expect(screen.queryByTestId("workgraph-action:child-2:claim")).toBeNull();
    expect(screen.queryByTestId("workgraph-attention:attention-1:pause")).toBeNull();
  });

  test("invokes provided callbacks with CAS revisions and gates buttons per row state", () => {
    const onClaim = vi.fn();
    const onClose = vi.fn();
    const onAttentionPause = vi.fn();
    render(
      <WorkGraphCard
        entry={entryFixture()}
        actions={{ onClaim, onClose, onAttentionPause }}
      />,
    );

    // Claim renders only for the open, unowned item.
    expect(screen.queryByTestId("workgraph-action:child-1:claim")).toBeNull();
    fireEvent.click(screen.getByTestId("workgraph-action:child-2:claim"));
    expect(onClaim).toHaveBeenCalledWith({ itemId: "child-2", revision: 1 });

    fireEvent.click(screen.getByTestId("workgraph-action:goal-1:close"));
    expect(onClose).toHaveBeenCalledWith({ itemId: "goal-1", revision: 4 });

    fireEvent.click(screen.getByTestId("workgraph-attention:attention-1:pause"));
    expect(onAttentionPause).toHaveBeenCalledWith({ bindingId: "attention-1", revision: 7 });

    // Resume never renders while the binding is active.
    expect(screen.queryByTestId("workgraph-attention:attention-1:resume")).toBeNull();
  });
});
