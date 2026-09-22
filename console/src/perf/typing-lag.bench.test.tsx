/**
 * Structural typing-lag benchmark for the MobKit console.
 *
 * Mounts the real ConsoleApp under jsdom with a fake transport: 160 agents in
 * the sidebar, one docked chat panel whose identity carries a transcript of
 * `N` markdown messages. Then it types 20 keystrokes into the composer and
 * pushes 20 SSE frames, recording wall time and component render counts.
 *
 * The assertions are ceilings that pin the structural fixes (composer draft
 * isolated from the app root, memoised siblings, memoised transcript
 * derivation, coalesced event frames). The timings are printed for the PR
 * body; only the render-count ceilings and a generous time bound are asserted
 * so the test stays deterministic on slow CI runners.
 */
import React from "react";
import { act, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { ConsoleApp } from "../ConsoleApp";
import type { MobKitConsoleTransport } from "../lib/headless";
import {
  installRenderCounts,
  resetRenderCounts,
  uninstallRenderCounts,
  type RenderCounts,
} from "../lib/render-counts";
import type { ConsoleFrame } from "../types";

const AGENT_COUNT = 160;
const CHAT_IDENTITY = "identity:agent-000";
const KEYSTROKES = 20;
const SSE_FRAMES = 20;

const MARKDOWN = [
  "Here is a **status** update with `inline code` and a [link](https://example.invalid).",
  "- first point\n- second point with *emphasis*\n- third point",
  "```rust\nfn main() {\n    println!(\"hello\");\n}\n```",
  "A longer paragraph that keeps going for a while so the row has some height and the markdown parser has some work to do before the next message arrives in the transcript.",
];

function agentRow(index: number) {
  const id = `identity:agent-${String(index).padStart(3, "0")}`;
  return {
    identity: id,
    member_id: id,
    agent_id: id,
    label: `Agent ${index}`,
    kind: "member",
    role: index % 4 === 0 ? "lead" : "worker",
    state: "running",
    addressable: true,
    affordances: { can_respawn: true, can_retire: true },
    model_capabilities: { image_input: false },
  };
}

function experience() {
  return {
    contract_version: "bench",
    runtime_id: "bench-runtime",
    console_config: {},
    console_policy: {},
    agent_sidebar: {
      live_snapshot: {
        agents: Array.from({ length: AGENT_COUNT }, (_, i) => agentRow(i)),
      },
    },
    activity_feed: { filter_presets: [], active_preset_id: "all" },
  };
}

function transcript(identity: string, count: number): ConsoleFrame[] {
  const frames: ConsoleFrame[] = [];
  const base = 1_700_000_000_000;
  for (let i = 0; i < count; i += 1) {
    const iid = `turn-${i}`;
    const ts = base + i * 10_000;
    frames.push({
      id: `${identity}:${i}:start`,
      event: "interaction_started",
      identity,
      interactionId: iid,
      timestampMs: ts,
      cursor: `console:${i * 2 + 1}`,
      data: { text: `Question number ${i}, please summarise.` },
    });
    frames.push({
      id: `${identity}:${i}:done`,
      event: "interaction_complete",
      identity,
      interactionId: iid,
      timestampMs: ts + 5_000,
      cursor: `console:${i * 2 + 2}`,
      data: { text: MARKDOWN[i % MARKDOWN.length] },
    });
  }
  return frames;
}

interface FakeTransport extends MobKitConsoleTransport {
  live?: (frame: ConsoleFrame) => void;
}

function fakeTransport(frames: ConsoleFrame[]): FakeTransport {
  const fake: FakeTransport = {
    loadExperience: async () => experience() as never,
    loadModules: async () => ({ modules: [] }) as never,
    capabilities: async () => ({ version: "bench", methods: [] }) as never,
    queryTimeline: async (input) => {
      const own = input.identity === CHAT_IDENTITY ? frames : [];
      return { frames: own, available: true } as never;
    },
    subscribeTimeline: (_input, onFrame) => {
      fake.live = onFrame;
      return () => {
        fake.live = undefined;
      };
    },
    send: async (input) =>
      ({ interaction_id: "sent", identity: input.identity, cursor: "console:x" }) as never,
    executeCommand: async (input) =>
      ({ command: input.command, accepted: true, result: {} }) as never,
    upload: async () => ({ blob_id: "blob" }) as never,
    blobUrl: (blobId) => `/blobs/${blobId}`,
  };
  return fake;
}

function seedDockedChat(baseUrl: string): void {
  // Mirrors ConsoleApp's dock persistence key and ConsoleDockState shape so the
  // app restores one focused chat panel for CHAT_IDENTITY at mount.
  const key = `mobkit-console-dock-state:bench-runtime`;
  const state = {
    tabs: [{ id: "tab-1", presetId: "single", layout: { kind: "panel", panelId: "panel-1" } }],
    panels: [
      {
        id: "panel-1",
        mode: "console",
        target: {
          id: `chat:${CHAT_IDENTITY}`,
          kind: "agent-chat",
          title: "Agent 0",
          identity: CHAT_IDENTITY,
          memberId: CHAT_IDENTITY,
        },
      },
    ],
    activeTabId: "tab-1",
    focusedPanelId: "panel-1",
  };
  window.localStorage.setItem(key, JSON.stringify(state));
  void baseUrl;
}

async function flush(): Promise<void> {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

async function settle(predicate: () => boolean, label: string, ms = 5_000): Promise<void> {
  const deadline = Date.now() + ms;
  while (!predicate()) {
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${label}`);
    await flush();
  }
}

interface Measurement {
  keystrokeMs: number[];
  keystrokeRenders: RenderCounts;
  frameMs: number[];
  frameRenders: RenderCounts;
}

function snapshotCounts(counts: RenderCounts): RenderCounts {
  return { ...counts };
}

async function measure(transcriptLength: number): Promise<Measurement> {
  const frames = transcript(CHAT_IDENTITY, transcriptLength);
  const transport = fakeTransport(frames);
  seedDockedChat("");
  const counts = installRenderCounts();
  const view = render(<ConsoleApp baseUrl="" transport={transport} />);
  await settle(
    () => view.container.querySelectorAll(".conv-turn").length >= Math.min(transcriptLength, 1),
    "transcript render",
  );
  const textarea = view.container.querySelector("textarea");
  if (!textarea) throw new Error("composer textarea not rendered");
  await flush();

  resetRenderCounts();
  const keystrokeMs: number[] = [];
  let draft = "";
  for (let i = 0; i < KEYSTROKES; i += 1) {
    draft += String.fromCharCode(97 + (i % 26));
    const started = performance.now();
    await act(async () => {
      fireEvent.change(textarea, { target: { value: draft } });
    });
    keystrokeMs.push(performance.now() - started);
  }
  const keystrokeRenders = snapshotCounts(counts);

  resetRenderCounts();
  const frameMs: number[] = [];
  const base = 1_800_000_000_000;
  for (let i = 0; i < SSE_FRAMES; i += 1) {
    const frame: ConsoleFrame = {
      id: `live:${i}`,
      event: "text_delta",
      identity: CHAT_IDENTITY,
      interactionId: "live-turn",
      timestampMs: base + i,
      cursor: `console:${100_000 + i}`,
      data: `token ${i} `,
    };
    const started = performance.now();
    await act(async () => {
      transport.live?.(frame);
    });
    frameMs.push(performance.now() - started);
  }
  // Let any coalescing scheduler drain before reading the frame counts.
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 50));
  });
  const frameRenders = snapshotCounts(counts);

  view.unmount();
  return { keystrokeMs, keystrokeRenders, frameMs, frameRenders };
}

function stats(samples: number[]): string {
  const sorted = [...samples].sort((a, b) => a - b);
  const mean = samples.reduce((a, b) => a + b, 0) / samples.length;
  const p95 = sorted[Math.min(sorted.length - 1, Math.floor(sorted.length * 0.95))];
  return `mean ${mean.toFixed(2)} ms, p95 ${p95.toFixed(2)} ms`;
}

describe("console typing lag benchmark", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });
  afterEach(() => {
    uninstallRenderCounts();
    window.localStorage.clear();
  });

  for (const n of [50, 500, 2000]) {
    it(`N=${n}: keystrokes do not re-render the app root, sidebar or transcript`, async () => {
      const m = await measure(n);
      // Printed for the PR body; the assertions below are the contract.
      console.log(
        `[typing-lag] N=${n} keystroke ${stats(m.keystrokeMs)} renders ${JSON.stringify(m.keystrokeRenders)}`,
      );
      console.log(
        `[typing-lag] N=${n} sse-frame ${stats(m.frameMs)} renders ${JSON.stringify(m.frameRenders)}`,
      );
      const perKeystroke = (name: string) => (m.keystrokeRenders[name] ?? 0) / KEYSTROKES;
      expect(perKeystroke("ConsoleApp")).toBe(0);
      expect(perKeystroke("Sidebar")).toBe(0);
      expect(perKeystroke("SignalsRail")).toBe(0);
      expect(perKeystroke("VoiceBar")).toBe(0);
      expect(perKeystroke("ChatPane")).toBe(0);
      expect(perKeystroke("TranscriptView")).toBe(0);
      // 20 frames within one animation frame budget each coalesce to far fewer
      // app renders than frames; the sidebar must not render per frame.
      expect(m.frameRenders["Sidebar"] ?? 0).toBeLessThanOrEqual(2);
      expect(m.frameRenders["ConsoleApp"] ?? 0).toBeLessThan(SSE_FRAMES);
      const meanKeystroke = m.keystrokeMs.reduce((a, b) => a + b, 0) / m.keystrokeMs.length;
      expect(meanKeystroke).toBeLessThan(25);
    }, 120_000);
  }
});
