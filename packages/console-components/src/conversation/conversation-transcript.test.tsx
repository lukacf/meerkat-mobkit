import { groupConversationTimelineEntries, type ConversationTimelineEntry, type ConversationViewState } from "@console-core";
import { fireEvent, render, screen } from "@testing-library/react";

import { ConversationTranscript } from "./conversation-transcript";

function Icon({ name }: { name: string; className?: string }) {
  return <span>{name}</span>;
}

function buildViewState(): ConversationViewState {
  const entries: ConversationTimelineEntry[] = [
    {
      id: "user-1",
      kind: "message",
      variant: "plain",
      identity: { id: "user", label: "You", role: "user" },
      text: "Ship the extraction.",
    },
    {
      id: "assistant-1",
      kind: "message",
      variant: "plain",
      identity: { id: "assistant", label: "Assistant", role: "assistant" },
      text: "I extracted the transcript and sidebar.",
    },
    {
      id: "assistant-2",
      kind: "summary",
      identity: { id: "assistant", label: "Assistant", role: "assistant" },
      title: "1 file changed",
      plus: 12,
      minus: 2,
      files: [{ name: "desktop/renderer/src/app/App.tsx", plus: 12, minus: 2 }],
    },
  ];

  return {
    conversationId: "thread-1",
    entries,
    groups: groupConversationTimelineEntries(entries),
    turnDiff: {
      fileCount: 1,
      plus: 12,
      minus: 2,
      files: [{
        path: "desktop/renderer/src/app/App.tsx",
        plus: 12,
        minus: 2,
        hunks: [{
          oldStart: 1,
          oldLines: 1,
          newStart: 1,
          newLines: 2,
          lines: [
            { type: "context", text: "import App from \"@/app/App\";", oldLine: 1, newLine: 1 },
            { type: "add", text: "import { ConversationTranscript } from \"@console-components\";", oldLine: null, newLine: 2 },
          ],
        }],
      }],
    },
    emptyState: null,
  };
}

describe("ConversationTranscript", () => {
  test("renders grouped transcript entries and expandable turn diffs", () => {
    const onToggleDiffFile = vi.fn();

    render(
      <ConversationTranscript
        Icon={Icon}
        expandedDiffFile={null}
        onToggleDiffFile={onToggleDiffFile}
        viewState={buildViewState()}
      />,
    );

    expect(screen.getByText("Ship the extraction.")).toBeInTheDocument();
    expect(screen.getByText("I extracted the transcript and sidebar.")).toBeInTheDocument();
    expect(screen.getAllByText("1 file changed")).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: /desktop\/renderer\/src\/app\/app\.tsx/i }));
    expect(onToggleDiffFile).toHaveBeenCalledWith("desktop/renderer/src/app/App.tsx");
  });

  test("renders labeled participant groups without app-specific transcript state", () => {
    const entries: ConversationTimelineEntry[] = [
      {
        id: "builder-1",
        kind: "message",
        variant: "plain",
        identity: {
          id: "builder",
          label: "Builder",
          role: "other",
          presentation: "participant",
          showLabel: true,
          meta: "implementation",
          avatarLabel: "BLD",
        },
        text: "I normalized the view model for MobKit.",
      },
    ];

    render(
      <ConversationTranscript
        Icon={Icon}
        onToggleDiffFile={() => undefined}
        viewState={{
          conversationId: "mobkit-thread",
          entries,
          groups: groupConversationTimelineEntries(entries),
          turnDiff: null,
          emptyState: null,
        }}
      />,
    );

    expect(screen.getByText("Builder")).toBeInTheDocument();
    expect(screen.getByText("implementation")).toBeInTheDocument();
    expect(screen.getByText("I normalized the view model for MobKit.")).toBeInTheDocument();
  });

  test("groups timeline entries into scrollable user turns", () => {
    const entries: ConversationTimelineEntry[] = [
      {
        id: "user-1",
        kind: "message",
        variant: "plain",
        identity: { id: "user", label: "You", role: "user" },
        text: "First request.",
      },
      {
        id: "assistant-1",
        kind: "message",
        variant: "plain",
        identity: { id: "assistant", label: "Assistant", role: "assistant" },
        text: "First response.",
      },
      {
        id: "user-2",
        kind: "message",
        variant: "plain",
        identity: { id: "user", label: "You", role: "user" },
        text: "Second request.",
      },
      {
        id: "assistant-2",
        kind: "message",
        variant: "plain",
        identity: { id: "assistant", label: "Assistant", role: "assistant" },
        text: "Second response.",
      },
    ];

    render(
      <ConversationTranscript
        viewState={{
          conversationId: "thread-turns",
          entries,
          groups: groupConversationTimelineEntries(entries),
          turnDiff: null,
          emptyState: null,
        }}
      />,
    );

    const turns = screen.getAllByTestId(/conversation-turn:/);
    expect(turns).toHaveLength(2);
    expect(turns[0]).toHaveTextContent("First request.");
    expect(turns[0]).toHaveTextContent("First response.");
    expect(turns[1]).toHaveTextContent("Second request.");
    expect(turns[1]).toHaveTextContent("Second response.");
  });
});
