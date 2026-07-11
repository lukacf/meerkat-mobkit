import {
  conversationEntryText,
  conversationIdentityGroupKey,
  groupConversationTimelineEntries,
  type ConversationTimelineEntry,
} from "./conversation";

describe("conversation grouping", () => {
  test("keeps adjacent entries from the same participant grouped together", () => {
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
        },
        text: "I started the refactor.",
      },
      {
        id: "builder-2",
        kind: "message",
        variant: "plain",
        identity: {
          id: "builder",
          label: "Builder",
          role: "other",
          presentation: "participant",
          showLabel: true,
        },
        text: "The transcript API is normalized now.",
      },
    ];

    const groups = groupConversationTimelineEntries(entries);

    expect(groups).toHaveLength(1);
    expect(groups[0]?.entries).toHaveLength(2);
    expect(conversationIdentityGroupKey(groups[0]?.identity)).toBe("builder:participant:label");
  });

  test("splits groups when adjacent entries change presentation", () => {
    const entries: ConversationTimelineEntry[] = [
      {
        id: "assistant-1",
        kind: "message",
        variant: "plain",
        identity: {
          id: "assistant",
          label: "Assistant",
          role: "assistant",
          presentation: "assistant",
          showLabel: false,
        },
        text: "I updated the shared package.",
      },
      {
        id: "assistant-2",
        kind: "message",
        variant: "meta",
        identity: {
          id: "assistant",
          label: "System",
          role: "assistant",
          presentation: "system",
          showLabel: true,
        },
        text: "Checkpoint saved.",
      },
    ];

    const groups = groupConversationTimelineEntries(entries);

    expect(groups).toHaveLength(2);
    expect(groups[0]?.identity.presentation).toBe("assistant");
    expect(groups[1]?.identity.presentation).toBe("system");
  });
});

describe("system task entries", () => {
  test("preserves the exact task prompt for transcript copy and accessibility surfaces", () => {
    const prompt = "Study the assigned domain.\n\nLearn every boundary and invariant.";
    const entry: ConversationTimelineEntry = {
      id: "system-task-domain-study",
      kind: "message",
      variant: "plain",
      identity: {
        id: "system-task-domain-study",
        label: "Initial domain study",
        role: "system",
        presentation: "system",
      },
      text: prompt,
      taskKind: "domain_reconnaissance",
      taskLabel: "Initial domain study",
      taskId: "domain-study-runtime",
      taskStatus: "running",
      runId: "run-domain-study-runtime",
    };

    expect(conversationEntryText(entry)).toBe(prompt);
    expect(conversationIdentityGroupKey(entry.identity)).toBe("system-task-domain-study:system:label");
  });
});

describe("flow run entries", () => {
  test("preserves an honest stopped state in the shared conversation model", () => {
    const entry: ConversationTimelineEntry = {
      id: "flow-run:stopped",
      kind: "flow_run",
      identity: { id: "coordinator", label: "Coordinator", role: "assistant" },
      helperId: "helper-1",
      flowName: "Release crew",
      status: "stopped",
      rows: [
        {
          memberKey: "reviewer",
          label: "Reviewer",
          caption: "Stopped by the operator",
          status: "stopped",
        },
      ],
    };

    expect(entry.status).toBe("stopped");
    expect(conversationEntryText(entry)).toContain("Reviewer: Stopped by the operator");
  });
});

describe("workgraph entry text", () => {
  test("projects the goal, indented item tree, and attention rows for copy surfaces", () => {
    const entry: ConversationTimelineEntry = {
      kind: "workgraph",
      id: "workgraph:goal-1",
      identity: { id: "planner", label: "Planner", role: "assistant" },
      rootId: "goal-1",
      title: "Release 0.7.30",
      objective: "Ship WorkGraph end to end",
      status: "active",
      progress: { completed: 1, total: 2 },
      items: [
        {
          itemId: "goal-1",
          title: "Release 0.7.30",
          status: "in_progress",
          revision: 4,
          depth: 0,
        },
        {
          itemId: "child-1",
          title: "Console card",
          status: "completed",
          revision: 2,
          depth: 1,
          parentId: "goal-1",
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
    };

    expect(conversationEntryText(entry)).toBe([
      "Release 0.7.30 (1/2)",
      "Ship WorkGraph end to end",
      "Release 0.7.30 — in progress",
      "  Console card — completed",
      "pursue: active → sess-42",
    ].join("\n"));
  });
});
