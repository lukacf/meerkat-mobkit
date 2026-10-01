/**
 * Real-browser typing-lag harness.
 *
 * Mounts the production ConsoleApp (same React build, same CSS, same DOM) with
 * an in-memory transport whose docked identity carries `?turns=N` turns of
 * real-shaped gateway frames (see realistic-transcript.ts): mixed markdown,
 * fenced code, GFM tables, long messages, tool calls, session-history twins
 * and history snapshots, paged like the gateway. `typing-lag-browser.cjs` loads this page in Chromium and
 * measures keystroke-to-next-paint latency in the composer. The transport is
 * the only thing faked; it is not on the keystroke path.
 */
import React from "react";
import { createRoot } from "react-dom/client";

import { ConsoleApp } from "../ConsoleApp";
import type { MobKitConsoleTransport } from "../lib/headless";
import * as adapters from "../lib/adapters";
import { parseSseFrames } from "../lib/network";
import type { ConsoleFrame } from "../types";
import { assistantReply, FixedTimeline, RealisticTimeline, type WireFrame } from "./realistic-transcript";
import { flowForensicsLikeSession, projectSessionHistory, type MeerkatSessionLike } from "./session-projection";

const params = new URLSearchParams(window.location.search);
const TURNS = Number(params.get("turns") ?? "1000");
const AGENT_COUNT = Number(params.get("agents") ?? "40");
const RUNTIME_ID = "perf-runtime";
const CHAT_IDENTITY = "router:main";

// ?session=flowforensics renders a session shaped like a real slow one;
// ?session=<url> projects a persisted meerkat Session JSON served locally.
const SESSION = params.get("session");
let timeline: RealisticTimeline | FixedTimeline = new RealisticTimeline(SESSION ? 0 : TURNS);
async function loadTimeline(): Promise<void> {
  if (!SESSION) return;
  const session: MeerkatSessionLike = SESSION === "flowforensics"
    ? flowForensicsLikeSession()
    : await (await fetch(SESSION)).json();
  timeline = new FixedTimeline(projectSessionHistory(session));
}

/// Gateway wire frame to ConsoleFrame through the console's own SSE parser.
function toConsoleFrames(frames: WireFrame[]): ConsoleFrame[] {
  return parseSseFrames(frames.map((frame) => `event: frame\ndata: ${JSON.stringify({ type: "frame", frame })}\n\n`).join(""));
}
let live: ((frame: ConsoleFrame) => void) | undefined;

function experience() {
  return {
    contract_version: "perf",
    runtime_id: RUNTIME_ID,
    console_config: {},
    console_policy: {},
    agent_sidebar: { live_snapshot: { agents: Array.from({ length: AGENT_COUNT }, (_, i) => agentRow(i)) } },
    activity_feed: { filter_presets: [], active_preset_id: "all" },
  };
}

function agentRow(index: number) {
  const id = index === 0 ? CHAT_IDENTITY : `identity:agent-${String(index).padStart(3, "0")}`;
  return {
    identity: id,
    member_id: id,
    agent_id: id,
    label: index === 0 ? "Router" : `Agent ${index}`,
    kind: "member",
    role: index % 4 === 0 ? "lead" : "worker",
    state: "running",
    addressable: true,
    affordances: { can_respawn: true, can_retire: true },
    model_capabilities: { image_input: false },
  };
}

const transport: MobKitConsoleTransport = {
  loadExperience: async () => experience() as never,
  loadModules: async () => ({ modules: [] }) as never,
  capabilities: async () => ({ version: "perf", methods: [] }) as never,
  // Pages exactly like the gateway: the newest `limit` frames before `before`.
  queryTimeline: async (input) => {
    if (input.identity !== CHAT_IDENTITY || input.mode === "since") {
      return { frames: [], available: true, exhausted: true } as never;
    }
    const page = timeline.recent(input.limit ?? 200, input.before);
    return { frames: toConsoleFrames(page.frames), available: true, exhausted: page.exhausted } as never;
  },
  subscribeTimeline: (_input, onFrame) => {
    live = onFrame;
    return () => {
      live = undefined;
    };
  },
  send: async (input) => ({ interaction_id: "sent", identity: input.identity, cursor: "console:x" }) as never,
  executeCommand: async (input) => ({ command: input.command, accepted: true, result: {} }) as never,
  upload: async () => ({ blob_id: "blob" }) as never,
  blobUrl: (blobId) => `/blobs/${blobId}`,
};

function seedDockedChat(): void {
  const panel = {
    id: "panel-1",
    mode: "console",
    target: { id: `chat:${CHAT_IDENTITY}`, kind: "agent-chat", title: "Router", identity: CHAT_IDENTITY, memberId: CHAT_IDENTITY },
  };
  window.localStorage.setItem(
    `mobkit-console-dock-state:${RUNTIME_ID}`,
    JSON.stringify({
      tabs: [{ id: "tab-1", presetId: "single", layout: { kind: "panel", panelId: panel.id } }],
      panels: [panel],
      activeTabId: "tab-1",
      focusedPanelId: panel.id,
    }),
  );
}

declare global {
  interface Window {
    __perf: {
      turns: number;
      frames: number;
      push(frame: ConsoleFrame): void;
      /** Stream a long reply as real-shaped text_delta frames, one every `everyMs`. */
      streamReply(everyMs: number, chunkChars?: number): () => void;
      /** Live frames delivered by streamReply so far. */
      streamed: number;
      /** Full and continued transcript derivations (absent on older trees). */
      derivations(): { full: number; extended: number } | null;
    };
  }
}

async function boot(): Promise<void> {
await loadTimeline();
window.__perf = {
  turns: TURNS,
  frames: timeline.frameCount,
  push: (frame) => live?.(frame),
  streamed: 0,
  derivations: () => {
    const stats = (adapters as unknown as { timelineDerivationStats?: { full: number; extended: number } }).timelineDerivationStats;
    return stats ? { ...stats } : null;
  },
  streamReply: (everyMs, chunkChars = 12) => {
    const text = Array.from({ length: 30 }, (_, i) => assistantReply(i)).join("\n\n");
    const frames = timeline.streamReply(text, chunkChars);
    let index = 0;
    const timer = window.setInterval(() => {
      const frame = frames[index++];
      if (!frame) return window.clearInterval(timer);
      window.__perf.streamed += 1;
      for (const parsed of toConsoleFrames([frame])) live?.(parsed);
    }, everyMs);
    return () => window.clearInterval(timer);
  },
};
window.localStorage.clear();
seedDockedChat();
createRoot(document.getElementById("root")!).render(<ConsoleApp baseUrl="" transport={transport} />);
}

void boot();
