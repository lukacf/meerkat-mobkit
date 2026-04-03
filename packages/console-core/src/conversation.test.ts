import {
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
