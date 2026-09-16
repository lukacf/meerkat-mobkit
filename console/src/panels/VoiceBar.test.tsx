import React from "react";
import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { VoiceSessionSnapshot } from "../lib/voice-session";
import { ChatPane } from "./ChatPane";
import { VoiceBar, VoiceButton } from "./VoiceBar";

const activeState: VoiceSessionSnapshot = {
  phase: "active",
  target: { identity: "identity:alpha", label: "Alpha" },
  microphoneMuted: false,
  speakerMuted: false,
  error: null,
  notice: null,
};
const sampleWaveform = vi.fn();
const controls = {
  sampleWaveform,
  onClose: vi.fn(),
  onToggleMicrophone: vi.fn(),
  onToggleSpeaker: vi.fn(),
};

describe("voice controls", () => {
  it("labels the persistent voice agent and exposes independent mute controls", () => {
    const view = render(<VoiceBar state={activeState} {...controls} />);
    expect(screen.getByRole("region", { name: "Voice with Alpha" })).toBeInTheDocument();
    expect(view.container.querySelectorAll("canvas")).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Mute microphone" }));
    fireEvent.click(screen.getByRole("button", { name: "Mute speakers" }));
    expect(controls.onToggleMicrophone).toHaveBeenCalledOnce();
    expect(controls.onToggleSpeaker).toHaveBeenCalledOnce();

    view.rerender(<VoiceBar state={{ ...activeState, microphoneMuted: true, speakerMuted: true }} {...controls} />);
    expect(screen.getByRole("button", { name: "Unmute microphone" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "Unmute speakers" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("status")).toHaveTextContent("Microphone muted");
    fireEvent.click(screen.getByRole("button", { name: "End voice conversation" }));
    expect(controls.onClose).toHaveBeenCalledOnce();
  });

  it("can cancel microphone permission or connection without enabling media controls", () => {
    const view = render(<VoiceBar state={{ ...activeState, phase: "requesting" }} {...controls} />);
    expect(screen.getByRole("status")).toHaveTextContent("Allow microphone access");
    expect(screen.getByRole("button", { name: "Mute microphone" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Mute speakers" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "End voice conversation" })).toBeEnabled();
    view.rerender(<VoiceBar state={{ ...activeState, phase: "connecting" }} {...controls} />);
    expect(screen.getByRole("status")).toHaveTextContent("Connecting");
  });

  it("shows startup errors and silence termination as dismissible non-modal messages", () => {
    const view = render(<VoiceBar state={{ ...activeState, phase: "error", error: "Microphone access was denied." }} {...controls} />);
    expect(screen.getByRole("alert")).toHaveTextContent("Microphone access was denied.");
    expect(view.container.querySelectorAll("canvas")).toHaveLength(0);
    expect(screen.getByRole("button", { name: "Dismiss voice" })).toBeEnabled();
    view.rerender(<VoiceBar state={{ ...activeState, phase: "idle", notice: "Voice ended after 15 minutes of silence." }} {...controls} />);
    expect(screen.getByText("Voice ended after 15 minutes of silence.")).toHaveAttribute("role", "status");
  });

  it("names the icon action and exposes its active state", () => {
    const onClick = vi.fn();
    const view = render(<VoiceButton agentLabel="Alpha" onClick={onClick} />);
    fireEvent.click(screen.getByRole("button", { name: "Start voice with Alpha" }));
    expect(onClick).toHaveBeenCalledOnce();
    view.rerender(<VoiceButton agentLabel="Alpha" active onClick={onClick} />);
    expect(screen.getByRole("button", { name: "End voice with Alpha" })).toHaveAttribute("aria-pressed", "true");
  });
});

describe("voice and text composer", () => {
  function props(label: string) {
    return {
      agent: { agent_id: label, member_id: label, label, kind: "identity", affordances: { can_send_message: true } },
      agentLabel: label,
      identity: `identity:${label.toLowerCase()}`,
      entries: [],
      phase: null,
      draft: "Keep working on the text task",
      sending: false,
      staged: [],
      onDraftChange: vi.fn(),
      onStagedChange: vi.fn(),
      onSend: vi.fn().mockResolvedValue(true),
      voiceSlot: <VoiceBar state={activeState} {...controls} />,
    };
  }

  it("keeps Alpha's voice controls above Beta's usable text input after navigation", () => {
    const alpha = props("Alpha");
    const view = render(<ChatPane {...alpha} onVoiceToggle={vi.fn()} voiceActive />);
    const beta = props("Beta");
    view.rerender(<ChatPane {...beta} onVoiceToggle={vi.fn()} />);
    expect(screen.getByRole("region", { name: "Voice with Alpha" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start voice with Beta" })).toBeInTheDocument();
    const input = screen.getByRole("textbox");
    expect(input).toBeEnabled();
    expect(input).toHaveAttribute("placeholder", "Message Beta…");
    expect(screen.getByTestId("voice-bar").compareDocumentPosition(input) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    fireEvent.click(screen.getByTestId("chat-send:identity:beta"));
    expect(beta.onSend).toHaveBeenCalledWith([]);
  });

  it("does not offer voice without the authenticated capability callback", () => {
    render(<ChatPane {...props("Alpha")} voiceSlot={null} />);
    expect(screen.queryByTestId("voice-start")).not.toBeInTheDocument();
    expect(screen.getByRole("textbox")).toBeEnabled();
  });

  it.each(["read-only", "send-denied"])("does not offer voice in a %s pane", (restriction) => {
    const pane = props("Alpha");
    render(
      <ChatPane
        {...pane}
        agent={{ ...pane.agent, affordances: { can_send_message: restriction !== "send-denied" } }}
        readOnly={restriction === "read-only"}
        accessEnforcing
        onVoiceToggle={vi.fn()}
      />,
    );
    expect(screen.queryByTestId("voice-start")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "End voice conversation" })).toBeEnabled();
  });
});
