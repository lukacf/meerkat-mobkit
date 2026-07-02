import { fireEvent, render, screen } from "@testing-library/react";
import { vi } from "vitest";

import {
  groupConversationTimelineEntries,
  type ConversationTimelineEntry,
  type ConversationViewState,
} from "@console-core";

import { ConversationPane } from "./conversation-pane";

function Icon({ name }: { name: string; className?: string }) {
  return <span>{name}</span>;
}

describe("ConversationPane", () => {
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
      expect(screen.getByText("Review the contract.")).toBeInTheDocument();
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
