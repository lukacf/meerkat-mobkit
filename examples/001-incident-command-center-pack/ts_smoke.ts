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
  data: unknown;
};

const scenario = YAML.parse(
  fs.readFileSync(path.join(repoRoot, "examples", "001-incident-command-center-pack", "scenario.yaml"), "utf8"),
) as { smoke?: { watched_identities?: string[] } };

const baseUrl = process.argv[2];
assert.ok(baseUrl, "baseUrl is required");

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
  const timeoutMs = init.timeoutMs ?? 6000;
  const until = init.until;
  const response = await fetch(url, init);
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
    timeoutMs: 15000,
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

const experience = await fetchJson<{
  contract_version: string;
  identity_status?: { rows?: unknown[] };
  activity_feed?: { filter_presets?: Array<{ id?: string }> };
}>(`${baseUrl}/console/experience`);

assert.equal(experience.contract_version, "0.3.0");
assert.ok(Array.isArray(experience.identity_status?.rows));
assert.ok(Array.isArray(experience.activity_feed?.filter_presets));
assert.ok(experience.activity_feed!.filter_presets!.some((preset) => preset.id === "watched-only"));

const { sendResult, frames } = await streamInteraction("incident-commander", "panel alpha follow-up", "ts-smoke:panel-1");
assert.ok(sendResult.interaction_id, "interaction_id expected");
assert.ok(frames.some((frame) => frame.event === "interaction_complete"), "terminal frame expected");
assert.ok(frames.some((frame) => frame.event === "text_delta"), "text delta frame expected");

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
