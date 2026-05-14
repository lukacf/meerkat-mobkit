import { fireEvent, render, screen, waitFor } from "@testing-library/react";

import type { ConversationTimelineEntry } from "@console-core";

import { ConversationMessageView } from "./conversation-message-view";

function Icon({ name }: { name: string; className?: string }) {
  return <span>{name}</span>;
}

describe("ConversationMessageView", () => {
  test("adds a copy affordance to user messages", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, {
      clipboard: {
        writeText,
      },
    });

    const entry: ConversationTimelineEntry = {
      id: "user-1",
      kind: "message",
      variant: "plain",
      identity: { id: "user", label: "You", role: "user" },
      text: "Please copy this question.",
    };

    render(<ConversationMessageView entry={entry} Icon={Icon} />);

    fireEvent.click(screen.getByRole("button", { name: /copy message/i }));

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith("Please copy this question.");
      expect(screen.getByRole("button", { name: /copied message/i })).toBeInTheDocument();
    });
  });

  test("renders rich user blocks instead of an empty user bubble", () => {
    const entry: ConversationTimelineEntry = {
      id: "user-image-1",
      kind: "message",
      variant: "rich",
      identity: { id: "user", label: "You", role: "user" },
      blocks: [{
        type: "image",
        src: "data:image/png;base64,ZmFrZQ==",
        mediaType: "image/png",
        alt: "uploaded receipt",
      }],
    };

    render(<ConversationMessageView entry={entry} Icon={Icon} />);

    expect(screen.getByRole("img", { name: /uploaded receipt/i })).toBeInTheDocument();
  });

  test("labels single outgoing peer tools with the concrete tool name", () => {
    const entry: ConversationTimelineEntry = {
      id: "peer-tool-1",
      kind: "message",
      variant: "rich",
      identity: { id: "agent", label: "Agent", role: "assistant" },
      blocks: [{
        type: "tool-call",
        toolCallId: "call-1",
        name: "send_message",
        arguments: "{\"peer_id\":\"peer-1\",\"body\":\"hello\"}",
        status: "success",
        peerTarget: "worker-a",
        peerBody: "hello",
      }],
    };

    render(<ConversationMessageView entry={entry} Icon={Icon} />);

    expect(screen.getByText("send_message → worker-a")).toBeInTheDocument();
  });

  test("deduplicates repeated incoming peer targets in grouped peer tool labels", () => {
    const entry: ConversationTimelineEntry = {
      id: "peer-tool-2",
      kind: "message",
      variant: "rich",
      identity: { id: "agent", label: "Agent", role: "assistant" },
      blocks: [
        {
          type: "tool-call",
          toolCallId: "call-1",
          name: "peer_message",
          arguments: "{}",
          status: "success",
          peerIncoming: true,
          peerTarget: "worker-a",
          peerBody: "first",
        },
        {
          type: "tool-call",
          toolCallId: "call-2",
          name: "peer_message",
          arguments: "{}",
          status: "success",
          peerIncoming: true,
          peerTarget: "worker-a",
          peerBody: "second",
        },
      ],
    };

    render(<ConversationMessageView entry={entry} Icon={Icon} />);

    expect(screen.getByText("Received from worker-a")).toBeInTheDocument();
    expect(screen.queryByText("Received from worker-a, worker-a")).not.toBeInTheDocument();
  });

  test("copies rich code blocks and flips the button into a copied state", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, {
      clipboard: {
        writeText,
      },
    });

    const entry: ConversationTimelineEntry = {
      id: "assistant-1",
      kind: "message",
      variant: "rich",
      identity: { id: "assistant", label: "Assistant", role: "assistant" },
      text: "const copied = true;",
      blocks: [{
        type: "code",
        language: "ts",
        body: "const copied = true;",
      }],
    };

    render(<ConversationMessageView entry={entry} Icon={Icon} />);

    fireEvent.click(screen.getByRole("button", { name: /copy code/i }));

    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith("const copied = true;");
      expect(screen.getByRole("button", { name: /copied code/i })).toBeInTheDocument();
    });
  });

  test("marks participant transcript entries with a participant presentation class", () => {
    const entry: ConversationTimelineEntry = {
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
      text: "I adapted the shared transcript for multi-member use.",
    };

    const { container } = render(<ConversationMessageView entry={entry} Icon={Icon} />);

    expect(container.querySelector(".cc-message--participant")).toBeTruthy();
  });
});
