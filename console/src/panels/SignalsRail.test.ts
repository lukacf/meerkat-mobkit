import assert from "node:assert/strict";
import test from "node:test";
import { buildSignalGroupsForTest } from "./SignalsRail";
import type { ConsoleFrame } from "../types";

test("signals rail deduplicates live and history copies of the same visible reply", () => {
  const frames: ConsoleFrame[] = [
    {
      id: "live-reply",
      event: "interaction_complete",
      identity: "incident-commander",
      interactionId: "turn-image-1",
      timestampMs: Date.now(),
      data: {
        result: "Image generation succeeded. Peer forwarding to scribe was sent with the generated image reference; ack not confirmed. +1 -0",
      },
    },
    {
      id: "scribe-comms",
      event: "system_notice",
      identity: "scribe",
      timestampMs: Date.now() - 1_000,
      sourceKind: "session_history",
      data: {
        kind: "comms",
        blocks: [{
          type: "comms",
          kind: "message",
          direction: "incoming",
          peer: {
            id: "incident-command-center/commander/incident-commander",
            display_name: "incident-command-center/commander/incident-commander",
          },
          request_id: "comms-1",
          content: [{ type: "text", text: "QA smoke: generated a synthetic CardinalPay status badge image." }],
        }],
      },
    },
    {
      id: "history-reply",
      event: "interaction_complete",
      identity: "incident-commander",
      interactionId: "turn-image-1",
      timestampMs: Date.now() - 2_000,
      sourceKind: "session_history",
      data: {
        message: {
          role: "block_assistant",
          blocks: [{
            block_type: "text",
            data: {
              text: "Image generation succeeded. Peer forwarding to scribe was sent with the generated image reference; ack not confirmed.",
            },
          }],
        },
      },
    },
  ];

  const groups = buildSignalGroupsForTest(frames);
  const commanderReplies = groups.filter((group) =>
    group.title === "Incident Commander replied"
    && group.detail.startsWith("Image generation succeeded")
  );

  assert.equal(commanderReplies.length, 1);
});

test("signals rail merges non-adjacent frames from the same turn", () => {
  const frames: ConsoleFrame[] = [
    {
      id: "reply",
      event: "interaction_complete",
      identity: "incident-commander",
      interactionId: "turn-1",
      timestampMs: Date.now(),
      data: { result: "Worker created successfully." },
    },
    {
      id: "other-agent",
      event: "interaction_complete",
      identity: "scribe",
      interactionId: "turn-scribe",
      timestampMs: Date.now() - 500,
      data: { result: "Logged the worker creation." },
    },
    {
      id: "prompt",
      event: "user_input",
      identity: "incident-commander",
      interactionId: "turn-1",
      timestampMs: Date.now() - 1_000,
      data: { content: "Create a worker." },
    },
  ];

  const groups = buildSignalGroupsForTest(frames);
  const commanderTurn = groups.find((group) => group.id === "interaction:turn-1");

  assert.equal(commanderTurn?.title, "Turn activity");
  assert.equal(commanderTurn?.items.length, 2);
});
