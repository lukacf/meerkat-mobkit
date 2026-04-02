import { render, screen } from "@testing-library/react";

import type { ConversationViewState } from "@console-core";

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
});
