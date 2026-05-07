import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import YAML from "yaml";

const repoRoot = path.resolve(path.dirname(new URL(import.meta.url).pathname), "..", "..");
const { parseSseFrames } = await import(path.join(repoRoot, "console", "dist", "index.cjs"));

type ConsoleFrame = {
  id?: string;
  event?: string;
  identity?: string;
  interactionId?: string;
  cursor?: string;
  status?: string;
  data: unknown;
};

const scenario = YAML.parse(
  fs.readFileSync(path.join(repoRoot, "examples", "001-incident-command-center-pack", "scenario.yaml"), "utf8"),
) as {
  smoke?: {
    watched_identities?: string[];
    prompts?: {
      tool_sweep?: string;
      alpha_follow_up?: string;
      bravo_follow_up?: string;
    };
  };
};

const baseUrl = process.argv[2];
assert.ok(baseUrl, "baseUrl is required");
const prompts = scenario.smoke?.prompts || {};

async function fetchJson<T>(url: string, init?: RequestInit): Promise<T> {
  const response = await fetch(url, init);
  assert.equal(response.ok, true, `request failed for ${url}`);
  return response.json() as Promise<T>;
}

async function callConsoleRpc<T>(
  method: string,
  params: Record<string, unknown>,
): Promise<T> {
  const response = await fetchJson<{ result?: T; error?: { code: number; message: string } }>(`${baseUrl}/console/rpc`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: `ts-smoke:${method}`,
      method,
      params,
    }),
  });
  if (response.error) {
    throw new Error(`${method} RPC error ${response.error.code}: ${response.error.message}`);
  }
  return response.result as T;
}

async function readSseFrames(
  url: string,
  init: RequestInit & { minFrames?: number; timeoutMs?: number; until?: (frame: ConsoleFrame) => boolean } = {},
): Promise<ConsoleFrame[]> {
  const minFrames = init.minFrames ?? 1;
  const timeoutMs = init.timeoutMs ?? 20000;
  const until = init.until;
  const controller = new AbortController();
  const response = await fetch(url, { ...init, signal: controller.signal });
  assert.equal(response.ok, true, `expected SSE response from ${url}`);
  assert.ok(response.body, `expected readable SSE body from ${url}`);

  const reader = response.body!.getReader();
  const decoder = new TextDecoder();
  let buffer = "";
  const frames: ConsoleFrame[] = [];
  const deadline = Date.now() + timeoutMs;

  try {
    while (Date.now() < deadline) {
      const remainingMs = Math.max(1, deadline - Date.now());
      const result = await Promise.race([
        reader.read(),
        new Promise<null>((resolve) => setTimeout(() => resolve(null), remainingMs)),
      ]);
      if (result === null) {
        break;
      }
      const { value, done } = result;
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const boundary = buffer.lastIndexOf("\n\n");
      if (boundary === -1) continue;
      const complete = buffer.slice(0, boundary + 2);
      buffer = buffer.slice(boundary + 2);
      for (const frame of parseSseFrames(complete) as ConsoleFrame[]) {
        frames.push(frame);
        if (frames.length >= minFrames && until?.(frame)) {
          return frames;
        }
      }
      if (frames.length >= minFrames && !until) {
        return frames;
      }
    }
  } finally {
    controller.abort();
    try {
      await reader.cancel();
    } catch {
      // best effort
    }
  }

  assert.ok(
    frames.length >= minFrames,
    `expected at least ${minFrames} SSE frames from ${url}, received ${frames.length}`,
  );
  return frames;
}

async function streamInteraction(identity: string, content: string, origin: string) {
  let acceptedInteractionId = "";
  const streamPromise = readSseFrames(`${baseUrl}/console/identity/stream`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ identity }),
    minFrames: 1,
    timeoutMs: 90000,
    until: (frame) =>
      frame.interactionId === acceptedInteractionId &&
      (frame.event === "interaction_complete" || frame.event === "interaction_failed"),
  });

  const sendResult = await callConsoleRpc<{ interaction_id: string; identity: string }>("mobkit/interact", {
    identity,
    content,
    origin,
  });
  acceptedInteractionId = sendResult.interaction_id;
  const frames = await streamPromise;
  const filtered = frames.filter((frame) => frame.interactionId === sendResult.interaction_id || frame.event === "subscribed");
  return { sendResult, frames: filtered };
}

async function streamConsoleSend(identity: string, content: string, origin: string) {
  const idempotencyKey = `${origin}:${Date.now().toString(36)}`;
  let acceptedInteractionId = "";
  const streamPromise = readSseFrames(`${baseUrl}/console/timeline/stream?identity=${encodeURIComponent(identity)}`, {
    minFrames: 2,
    timeoutMs: 90000,
    until: (frame) =>
      frame.interactionId === acceptedInteractionId &&
      (frame.event === "interaction_complete" || frame.event === "interaction_failed"),
  });

  const accepted = await callConsoleRpc<{
    interaction_id: string;
    identity: string;
    input_frame_id: string;
    cursor: string;
    status: string;
  }>("mobkit/console/send", {
    identity,
    content,
    origin,
    idempotency_key: idempotencyKey,
    handling_mode: "queue",
  });
  acceptedInteractionId = accepted.interaction_id;
  assert.equal(accepted.identity, identity);
  assert.ok(accepted.input_frame_id, "canonical input frame id expected");
  assert.ok(accepted.cursor?.startsWith("console:"), "aggregate cursor expected");
  assert.ok(["accepted", "dispatching", "delivered"].includes(accepted.status), "send status expected");

  const frames = await streamPromise;
  const matchingFrames = frames.filter((frame) => frame.interactionId === accepted.interaction_id);
  assert.ok(matchingFrames.some((frame) => frame.event === "user_input"), "canonical user_input frame expected");
  assert.ok(matchingFrames.some((frame) => frame.event === "frame_updated"), "replayable frame_updated marker expected");
  assert.ok(matchingFrames.some((frame) => frame.event === "interaction_complete"), "canonical terminal frame expected");
  return { accepted, frames: matchingFrames };
}

async function main() {
  const experience = await fetchJson<{
    contract_version: string;
    identity_status?: { rows?: unknown[] };
    activity_feed?: { filter_presets?: Array<{ id?: string }> };
  }>(`${baseUrl}/console/experience`);

  assert.equal(experience.contract_version, "0.3.0");
  assert.ok(Array.isArray(experience.identity_status?.rows));
  assert.ok(Array.isArray(experience.activity_feed?.filter_presets));
  assert.ok(experience.activity_feed!.filter_presets!.some((preset) => preset.id === "watched-only"));

  const { sendResult, frames } = await streamInteraction(
    "incident-commander",
    prompts.tool_sweep || "Run a status sweep. Use both tools before answering.",
    "ts-smoke:tool-sweep",
  );
  assert.ok(sendResult.interaction_id, "interaction_id expected");
  assert.ok(frames.some((frame) => frame.event === "interaction_complete"), "terminal frame expected");
  assert.ok(frames.some((frame) => frame.event === "text_delta"), "text delta frame expected");
  assert.ok(frames.some((frame) => frame.event === "tool_call_requested"), "tool call expected");
  assert.ok(frames.some((frame) => frame.event === "tool_result_received"), "tool result expected");
  assert.ok(
    frames.some((frame) => frame.data && JSON.stringify(frame.data).includes("inspect_service")),
    "inspect_service tool usage expected",
  );
  assert.ok(
    frames.some((frame) => frame.data && JSON.stringify(frame.data).includes("analyze_customer_impact")),
    "analyze_customer_impact tool usage expected",
  );
  const toolCallIndex = frames.findIndex((frame) => frame.event === "tool_call_requested");
  const toolResultIndex = frames.findIndex((frame) => frame.event === "tool_result_received");
  const textDeltaIndex = frames.findIndex((frame) => frame.event === "text_delta");
  const terminalIndex = frames.findIndex((frame) => frame.event === "interaction_complete");
  assert.ok(toolCallIndex >= 0, "tool call frame index expected");
  assert.ok(toolResultIndex > toolCallIndex, "tool result should follow tool call");
  assert.ok(textDeltaIndex > toolResultIndex, "text generation should start after tool results");
  assert.ok(terminalIndex > textDeltaIndex, "terminal event should follow text generation");

  const canonicalTurn = await streamConsoleSend(
    "incident-commander",
    `Console substrate canonical send smoke. Reply with exactly OK. [${Date.now().toString(36)}:canonical]`,
    "ts-smoke:console-send",
  );
  const timelinePage = await callConsoleRpc<{ frames: ConsoleFrame[]; next_cursor?: string }>(
    "mobkit/console/query_timeline",
    {
      identity: "incident-commander",
      after: canonicalTurn.accepted.cursor,
      limit: 20,
    },
  );
  assert.ok(
    timelinePage.frames.some((frame) => frame.event === "frame_updated" || frame.event === "interaction_complete"),
    "query_timeline should replay aggregate frames after the accepted cursor",
  );

  const checkpointFrame = frames.find((frame) => frame.id && frame.event === "text_delta");
  assert.ok(checkpointFrame?.id, "checkpoint frame expected");
  const identityReplay = await readSseFrames(`${baseUrl}/console/identity/stream`, {
    method: "POST",
    headers: {
      "content-type": "application/json",
      "Last-Event-ID": checkpointFrame.id!,
    },
    body: JSON.stringify({ identity: "incident-commander" }),
    minFrames: 2,
  });
  assert.ok(identityReplay.every((frame) => frame.id !== checkpointFrame.id), "identity replay must resume after checkpoint");

  const allEventsFrames = await readSseFrames(`${baseUrl}/console/events/stream`, {
    headers: { "Last-Event-ID": checkpointFrame.id! },
    minFrames: 2,
  });
  assert.ok(allEventsFrames.length > 0, "all-events replay frames expected");
  assert.ok(allEventsFrames.every((frame) => frame.id !== checkpointFrame.id), "all-events replay must resume after checkpoint");

  const [alphaTurn, bravoTurn] = await Promise.all([
    streamInteraction(
      "incident-commander",
      prompts.alpha_follow_up || "Panel alpha follow-up. Give one short sentence about rollback guardrails.",
      "ts-smoke:panel-alpha",
    ),
    streamInteraction(
      "incident-commander",
      prompts.bravo_follow_up || "Panel bravo follow-up. Give one short sentence about customer impact.",
      "ts-smoke:panel-bravo",
    ),
  ]);

  assert.notEqual(alphaTurn.sendResult.interaction_id, bravoTurn.sendResult.interaction_id, "distinct interaction ids expected");
  assert.ok(
    alphaTurn.frames
      .filter((frame) => frame.event !== "subscribed")
      .every((frame) => frame.interactionId === alphaTurn.sendResult.interaction_id),
    "alpha stream must only surface alpha interaction frames",
  );
  assert.ok(
    bravoTurn.frames
      .filter((frame) => frame.event !== "subscribed")
      .every((frame) => frame.interactionId === bravoTurn.sendResult.interaction_id),
    "bravo stream must only surface bravo interaction frames",
  );
  assert.ok(alphaTurn.frames.some((frame) => frame.event === "interaction_complete"), "alpha terminal frame expected");
  assert.ok(bravoTurn.frames.some((frame) => frame.event === "interaction_complete"), "bravo terminal frame expected");

  const internalReject = await fetchJson<{ error?: { code?: number } }>(`${baseUrl}/console/rpc`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: "ts-smoke-reject",
      method: "mobkit/interact",
      params: {
        identity: "approval-gate",
        content: "should reject",
        origin: "ts-smoke",
      },
    }),
  });
  assert.equal(internalReject.error?.code, -32002);

  const statusIdentity = await callConsoleRpc<{ identity: string; addressability: string }>(
    "mobkit/status_identity",
    { identity: "incident-commander" },
  );
  assert.equal(statusIdentity.identity, "incident-commander");
  assert.equal(statusIdentity.addressability, "addressable");
  assert.ok((scenario.smoke?.watched_identities || []).includes("incident-commander"));

  console.log("incident TS smoke passed");
}

await main();
process.exit(0);
