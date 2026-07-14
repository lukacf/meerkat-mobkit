// NOTE (2026-07-14 audit): this file is written against vitest globals and
// @testing-library/react, NEITHER of which is a dependency of this repo — it
// has never executed in any CI lane or local runner. Runnable coverage for
// these behaviors lives in console/src/lib/component-interaction.test.tsx
// (repo-standard esbuild + node --test) and, for the pure grouping/parsing
// logic, in packages/console-core/src/*.test.ts via the node:test-backed
// shim (test-support/vitest-shim.ts). If you add vitest + RTL as real
// dependencies, wire this file into CI before trusting it.
import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";

import {
  groupConversationTimelineEntries,
  type ConversationFlowRunEntry,
  type ConversationTimelineEntry,
  type ConversationViewState,
} from "@console-core";

import { ConversationPane } from "./conversation-pane";

function Icon({ name }: { name: string; className?: string }) {
  return <span>{name}</span>;
}

describe("ConversationPane", () => {
  test("renders the scroll tail inside the scrolling body and keeps it out of the footer", () => {
    const viewState: ConversationViewState = {
      conversationId: "thread-scroll-tail",
      entries: [],
      groups: [],
      turnDiff: null,
      emptyState: null,
    };

    render(
      <ConversationPane
        footer={<div data-testid="pane-footer-content">Footer</div>}
        scrollTail={<div data-testid="pane-scroll-tail">Scroll tail</div>}
        viewState={viewState}
      />,
    );

    const scrollTail = screen.getByTestId("pane-scroll-tail");
    const scrollContainer = scrollTail.closest(".cc-conversation-pane__scroll");
    const body = scrollTail.closest(".cc-conversation-pane__body");
    const footer = screen.getByTestId("pane-footer-content").closest(".cc-conversation-pane__footer");

    expect(scrollContainer).toContainElement(scrollTail);
    expect(body?.lastElementChild).toBe(scrollTail);
    expect(footer).not.toContainElement(scrollTail);
  });

  test("restores the specific flow-run card that was clicked", () => {
    const releaseCrew: ConversationFlowRunEntry = {
      id: "flow-run:release-crew",
      kind: "flow_run",
      identity: { id: "coordinator", label: "Coordinator", role: "assistant" },
      helperId: "helper-release",
      flowName: "Release crew",
      status: "stopped",
      restorable: true,
      rows: [],
    };
    const reviewCrew: ConversationFlowRunEntry = {
      id: "flow-run:review-crew",
      kind: "flow_run",
      identity: { id: "coordinator", label: "Coordinator", role: "assistant" },
      helperId: "helper-review",
      flowName: "Review crew",
      status: "stopped",
      restorable: true,
      rows: [],
    };
    const entries: ConversationTimelineEntry[] = [releaseCrew, reviewCrew];
    const onFlowRunRestore = vi.fn();

    render(
      <ConversationPane
        onFlowRunRestore={onFlowRunRestore}
        viewState={{
          conversationId: "thread-restorable-crews",
          entries,
          groups: groupConversationTimelineEntries(entries),
          turnDiff: null,
          emptyState: null,
        }}
      />,
    );

    const restoreButtons = screen.getAllByRole("button", { name: "Resume" });
    expect(restoreButtons).toHaveLength(2);

    fireEvent.click(restoreButtons[0]);
    fireEvent.click(restoreButtons[1]);

    expect(onFlowRunRestore).toHaveBeenNthCalledWith(1, "helper-release", releaseCrew);
    expect(onFlowRunRestore).toHaveBeenNthCalledWith(2, "helper-review", reviewCrew);
  });

  test("falls back to the empty state when turn diffs cannot be rendered", () => {
    const viewState: ConversationViewState = {
      conversationId: "thread-1",
      title: "New thread",
      entries: [],
      groups: [],
      turnDiff: {
        fileCount: 1,
        plus: 12,
        minus: 3,
        files: [{
          path: "desktop/renderer/src/app/App.tsx",
          plus: 12,
          minus: 3,
          hunks: [],
        }],
      },
      emptyState: {
        title: "New thread",
        subtitle: "Ask Meerkat to do something in this workspace.",
        projectLabel: "workspace",
        iconName: "i-cube",
        suggestions: [],
      },
    };

    render(<ConversationPane Icon={Icon} viewState={viewState} />);

    expect(screen.getByText("New thread")).toBeInTheDocument();
    expect(screen.getByText("Ask Meerkat to do something in this workspace.")).toBeInTheDocument();
  });

  test("renders turn rail markers with previews and jumps to a selected turn", () => {
    const scrollIntoView = vi.fn();
    const previousScrollIntoView = HTMLElement.prototype.scrollIntoView;
    HTMLElement.prototype.scrollIntoView = scrollIntoView;

    const entries: ConversationTimelineEntry[] = [
      {
        id: "user-1",
        kind: "message",
        variant: "plain",
        identity: { id: "user", label: "You", role: "user" },
        text: "Review the contract.",
      },
      {
        id: "assistant-1",
        kind: "message",
        variant: "plain",
        identity: { id: "assistant", label: "Assistant", role: "assistant" },
        text: "Done. I left a focused finding.",
      },
      {
        id: "summary-1",
        kind: "summary",
        identity: { id: "assistant", label: "Assistant", role: "assistant" },
        title: "2 files changed",
        plus: 10,
        minus: 1,
        files: [
          { name: "agent-memory-architecture.mdx", plus: 8, minus: 0 },
          { name: "docs.json", plus: 2, minus: 1 },
        ],
      },
      {
        id: "user-2",
        kind: "message",
        variant: "plain",
        identity: { id: "user", label: "You", role: "user" },
        text: "Write up the design.",
      },
      {
        id: "assistant-2",
        kind: "message",
        variant: "plain",
        identity: { id: "assistant", label: "Assistant", role: "assistant" },
        text: "Done. I added the design page.",
      },
    ];

    try {
      render(
        <ConversationPane
          Icon={Icon}
          viewState={{
            conversationId: "thread-rail",
            entries,
            groups: groupConversationTimelineEntries(entries),
            turnDiff: null,
            emptyState: null,
          }}
        />,
      );

      expect(screen.getByRole("navigation", { name: "Conversation turns" })).toBeInTheDocument();
      // The rail preview repeats the opening user line, so the text appears in
      // both the preview card and the transcript itself.
      expect(screen.getAllByText("Review the contract.").length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText("Done. I left a focused finding.").length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText("agent-memory-architecture.mdx").length).toBeGreaterThanOrEqual(1);
      expect(screen.getAllByText("docs.json").length).toBeGreaterThanOrEqual(1);

      fireEvent.click(screen.getByTestId("conversation-turn-rail:1"));

      expect(scrollIntoView).toHaveBeenCalledWith({
        block: "start",
        behavior: "smooth",
      });
    } finally {
      HTMLElement.prototype.scrollIntoView = previousScrollIntoView;
    }
  });
});
