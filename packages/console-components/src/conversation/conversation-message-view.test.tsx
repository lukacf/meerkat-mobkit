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

  test("renders the exact initial-domain-study prompt in an accessible collapsible system card", () => {
    const prompt = "Study the runtime domain.\n\nLearn every boundary, invariant, and workflow.";
    const entry: ConversationTimelineEntry = {
      id: "system-task-domain-study",
      kind: "message",
      variant: "plain",
      identity: {
        // Typed task metadata is authoritative even if a consumer's generic
        // transcript adapter would otherwise present this entry as a user turn.
        id: "user",
        label: "You",
        role: "user",
        presentation: "user",
        showLabel: false,
      },
      text: prompt,
      taskKind: "domain_reconnaissance",
      taskLabel: "Initial domain study",
      taskId: "domain-study-runtime",
      taskStatus: "running",
      runId: "run-domain-study-runtime",
    };

    const { container } = render(<ConversationMessageView entry={entry} Icon={Icon} />);

    const card = screen.getByRole("group", { name: "Initial domain study" });
    expect(card).toBeInstanceOf(HTMLDetailsElement);
    expect(card).not.toHaveAttribute("open");
    expect(card).toHaveClass("cc-message--system-task", "cc-message--system");
    expect(card).not.toHaveClass("cc-message--user");
    expect(screen.queryByRole("button", { name: /copy message/i })).not.toBeInTheDocument();
    expect(screen.getByText("Initial domain study", { selector: "summary span" })).toBeInTheDocument();
    expect(screen.getByText("Domain reconnaissance · Running", { exact: false })).toBeInTheDocument();
    expect(container.querySelector(".cc-rich-thinking__body")?.textContent).toBe(prompt);

    fireEvent.click(screen.getByText("Initial domain study", { selector: "summary span" }));
    expect(card).toHaveAttribute("open");
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

    // The transcript keeps peer sends humanized: the header carries the peer
    // target, never the raw tool name (mobkit's pre-union `send_message →
    // worker-a` label was superseded by the studio's title-neutral rendering).
    expect(screen.getByText("worker-a")).toBeInTheDocument();
    expect(screen.queryByText(/send_message/)).not.toBeInTheDocument();
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

  test("collapses persisted thinking summaries by default", () => {
    const entry: ConversationTimelineEntry = {
      id: "assistant-thinking",
      kind: "message",
      variant: "rich",
      identity: { id: "assistant", label: "Assistant", role: "assistant" },
      text: "Used project context before answering.",
      blocks: [{
        type: "thinking",
        label: "Thinking Summary",
        text: "Used project context before answering.",
        final: true,
        persisted: true,
      }],
    };

    const { container } = render(<ConversationMessageView entry={entry} Icon={Icon} />);

    const thinking = container.querySelector("details.cc-rich-thinking");
    expect(thinking).toBeInTheDocument();
    expect(thinking).not.toHaveAttribute("open");
    expect(screen.getByText("Thinking Summary")).toBeInTheDocument();
  });

  test("hides machine peer intents when peer messages have readable bodies", () => {
    const entry: ConversationTimelineEntry = {
      id: "assistant-peer",
      kind: "message",
      variant: "rich",
      identity: { id: "assistant", label: "Assistant", role: "assistant" },
      text: "Hello from the app thread.",
      blocks: [{
        type: "tool-call",
        toolCallId: "peer-1",
        name: "send_message",
        arguments: JSON.stringify({
          peer_id: "peer-lib",
          handling_mode: "steer",
          body: "Hello from the app thread.",
          params: { subject: "peer-merge-123" },
        }),
        result: "completed",
        status: "success",
        peerTarget: "Lib thread",
        peerIntent: "steer",
        peerBody: "Hello from the app thread.",
      }],
    };

    render(<ConversationMessageView entry={entry} Icon={Icon} />);

    expect(screen.getByText("Lib thread")).toBeInTheDocument();
    expect(screen.getByText("Hello from the app thread.")).toBeInTheDocument();
    expect(screen.queryByText("steer")).not.toBeInTheDocument();

    fireEvent.click(screen.getByText("Lib thread"));

    expect(screen.getByText("Message")).toBeInTheDocument();
    expect(screen.getAllByText("Hello from the app thread.")).toHaveLength(2);
    expect(screen.queryByText(/peer-lib/)).not.toBeInTheDocument();
    expect(screen.queryByText(/handling_mode/)).not.toBeInTheDocument();
    expect(screen.queryByText(/peer-merge/)).not.toBeInTheDocument();
    expect(screen.queryByText(/completed/)).not.toBeInTheDocument();
  });

  test("renders persisted raw UUID peer targets as a generic peer label", () => {
    const entry: ConversationTimelineEntry = {
      id: "assistant-peer-uuid",
      kind: "message",
      variant: "rich",
      identity: { id: "assistant", label: "Assistant", role: "assistant" },
      text: "Response token delivered.",
      blocks: [{
        type: "tool-call",
        toolCallId: "peer-uuid",
        name: "send_response",
        arguments: JSON.stringify({ peer_id: "e3ec9e90-460e-51b3-80b9-dea0f0c31752" }),
        status: "success",
        peerTarget: "e3ec9e90-460e-51b3-80b9-dea0f0c31752",
        peerBody: "Response token delivered.",
      }],
    };

    render(<ConversationMessageView entry={entry} Icon={Icon} />);

    expect(screen.getByText("Peer")).toBeInTheDocument();
    expect(screen.queryByText("e3ec9e90-460e-51b3-80b9-dea0f0c31752")).not.toBeInTheDocument();
  });

  test("summarizes legacy MobKit peer protocol prompts in peer cards", () => {
    const entry: ConversationTimelineEntry = {
      id: "assistant-peer-protocol-body",
      kind: "message",
      variant: "rich",
      identity: { id: "assistant", label: "Assistant", role: "assistant" },
      text: "Response requested.",
      blocks: [{
        type: "tool-call",
        toolCallId: "peer-protocol-body",
        name: "send_request",
        arguments: JSON.stringify({ body: 'Please send_response with result.token exactly "peer-merge-123".' }),
        status: "success",
        peerIncoming: true,
        peerTarget: "HSNS thread",
        peerBody: 'Please send_response with result.token exactly "peer-merge-123".',
      }],
    };

    render(<ConversationMessageView entry={entry} Icon={Icon} />);

    expect(screen.getByText("Response requested.")).toBeInTheDocument();
    expect(screen.queryByText(/send_response/)).not.toBeInTheDocument();
  });
});
