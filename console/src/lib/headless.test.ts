import assert from "node:assert/strict";
import test from "node:test";

import { migrateConsoleWorkbenchTarget, type ConsoleWorkbenchTarget } from "@console-core";
import { CONSOLE_REST_PATHS, CONSOLE_RPC_METHODS, CONSOLE_RPC_PATHS } from "./contract";
import {
  createHttpConsoleTransport,
  createMobKitConsoleController,
  type ConsoleCapabilities,
  type MobKitConsoleTransport,
} from "./headless";
import type { ConsoleFrame, ConsoleTimelineAccepted, ConsoleTimelinePage } from "../types";

function identityTarget(): ConsoleWorkbenchTarget {
  const target = migrateConsoleWorkbenchTarget({
    id: "chat:identity:lead",
    kind: "mobkit/identity-chat",
    title: "Lead",
    identity: "identity:lead",
  });
  assert.ok(target);
  return target;
}

function hostTarget(): ConsoleWorkbenchTarget {
  const target = migrateConsoleWorkbenchTarget({
    id: "project:alpha",
    kind: "host/project",
    title: "Project Alpha",
    payloadVersion: 1,
    payload: { projectId: "alpha" },
  });
  assert.ok(target);
  return target;
}

function createFakeTransport(options: {
  capabilities?: ConsoleCapabilities;
  queryPages?: ConsoleTimelinePage[];
  accepted?: ConsoleTimelineAccepted;
} = {}): MobKitConsoleTransport & {
  live?: (frame: ConsoleFrame) => void;
  sends: unknown[];
  subscriptions: unknown[];
} {
  const queryPages = [...(options.queryPages || [])];
  const fake = {
    sends: [] as unknown[],
    subscriptions: [] as unknown[],
    loadExperience: async () => ({ contract_version: "fake" }),
    capabilities: async () => options.capabilities || {
      version: "fake-capabilities",
      methods: [CONSOLE_RPC_METHODS.send, "mobkit/identity/inspect"],
    },
    queryTimeline: async () => queryPages.shift() || { frames: [], available: true },
    subscribeTimeline: (input, onFrame) => {
      fake.subscriptions.push(input);
      fake.live = onFrame;
      return () => {
        fake.live = undefined;
      };
    },
    send: async (input) => {
      fake.sends.push(input);
      return options.accepted || {
        interaction_id: "turn-1",
        identity: input.identity,
        cursor: "console:2",
      };
    },
    executeCommand: async (input) => ({
      command: input.command,
      accepted: true,
      result: { ok: true },
    }),
    upload: async () => ({ blob_id: "blob-1" }),
    blobUrl: (blobId) => `/blobs/${blobId}`,
  } satisfies MobKitConsoleTransport & {
    live?: (frame: ConsoleFrame) => void;
    sends: unknown[];
    subscriptions: unknown[];
  };
  return fake;
}

test("headless timeline controller seeds, subscribes after cursor, backfills replay gaps, and deduplicates frames", async () => {
  const seed: ConsoleFrame = { id: "seed", event: "interaction_started", cursor: "console:1", data: {} };
  const backfill: ConsoleFrame = { id: "backfill", event: "text_delta", cursor: "console:2", data: "a" };
  const live: ConsoleFrame = { id: "live", event: "text_delta", cursor: "console:3", data: "b" };
  const transport = createFakeTransport({
    queryPages: [
      { frames: [seed], available: true, latestCursor: "console:1" },
      { frames: [seed, backfill], available: true, latestCursor: "console:2" },
    ],
  });
  const controller = createMobKitConsoleController({ transport });
  const delivered: ConsoleFrame[] = [];

  const unsubscribe = await controller.timeline.subscribeWithBackfill(
    { identity: "identity:lead" },
    (frame) => delivered.push(frame.value),
  );

  assert.deepEqual(delivered.map((frame) => frame.id), ["seed"]);
  assert.deepEqual(transport.subscriptions, [{ identity: "identity:lead", after: "console:1" }]);

  transport.live?.({ id: "gap", event: "replay_unavailable", data: {} });
  await new Promise((resolve) => setTimeout(resolve, 0));
  transport.live?.(live);
  unsubscribe();
  transport.live?.({ id: "after-unsubscribe", event: "text_delta", data: "ignored" });

  assert.deepEqual(delivered.map((frame) => frame.id), ["seed", "backfill", "live"]);
});

test("headless command surface sends only capability-gated MobKit identity targets and returns optimistic plus accepted facts", async () => {
  const transport = createFakeTransport();
  const controller = createMobKitConsoleController({ transport });

  const result = await controller.commands.sendMessage(identityTarget(), {
    content: "hello",
    origin: "test",
    idempotencyKey: "idem-1",
    handlingMode: "queue",
    attachments: [{} as File],
  });

  assert.equal(result.optimistic.provenance.source, "optimistic");
  assert.equal(result.optimistic.provenance.correlationId, "idem-1");
  assert.equal(result.accepted.provenance.source, "mobkit-protocol");
  assert.equal(result.accepted.provenance.routeOrMethod, CONSOLE_RPC_METHODS.send);
  assert.equal(result.accepted.provenance.capabilityVersion, "fake-capabilities");
  assert.equal(transport.sends.length, 1);
  assert.equal((transport.sends[0] as { identity?: string }).identity, "identity:lead");
});

test("headless command surface fails closed on missing capabilities and inert host targets", async () => {
  const noSend = createMobKitConsoleController({
    transport: createFakeTransport({ capabilities: { methods: [] } }),
  });

  await assert.rejects(
    () => noSend.commands.sendMessage(identityTarget(), {
      content: "hello",
      origin: "test",
      idempotencyKey: "idem-2",
    }),
    /capability missing/i,
  );

  const controller = createMobKitConsoleController({ transport: createFakeTransport() });
  await assert.rejects(
    () => controller.commands.sendMessage(hostTarget(), {
      content: "hello",
      origin: "test",
      idempotencyKey: "idem-3",
    }),
    /cannot send/i,
  );
  for (const command of [
    CONSOLE_RPC_METHODS.send,
    "mobkit/member/retire",
    "mobkit/member/respawn",
    "mobkit/member/reset",
    "mobkit/routing/routes/list",
    "mobkit/gating/decide",
    "mobkit/raw/rpc",
  ]) {
    await assert.rejects(
      () => controller.commands.execute({
        command,
        target: hostTarget(),
      }),
      /host target .* cannot execute/i,
    );
  }
});

test("headless transport keeps uploads and blob URLs typed optional hooks", async () => {
  const transport = createFakeTransport();
  const upload = await transport.upload?.({ file: {} as File, mediaType: "image/png" });

  assert.deepEqual(upload, { blob_id: "blob-1" });
  assert.equal(transport.blobUrl?.("blob-1"), "/blobs/blob-1");
});

test("headless facts carry all required provenance classes", () => {
  const controller = createMobKitConsoleController({ transport: createFakeTransport() });

  assert.equal(controller.facts.mobkit({}).provenance.source, "mobkit-protocol");
  assert.equal(controller.facts.derived({}).provenance.source, "controller-derived");
  assert.equal(controller.facts.optimistic({}, "idem").provenance.source, "optimistic");
  assert.equal(controller.facts.host({}).provenance.source, "host-adapter");
});

test("createHttpConsoleTransport uses stock console routes and typed RPC methods", async () => {
  const calls: Array<{ url: string; method: string; body?: string }> = [];
  const previousFetch = globalThis.fetch;
  globalThis.fetch = (async (url: string | URL | Request, init?: RequestInit) => {
    const requestUrl = String(url);
    calls.push({
      url: requestUrl,
      method: init?.method || "GET",
      body: typeof init?.body === "string" ? init.body : undefined,
    });
    if (requestUrl.endsWith(CONSOLE_REST_PATHS.experience)) {
      return new Response(JSON.stringify({ contract_version: "0.5.0" }), { status: 200 });
    }
    if (requestUrl.endsWith(CONSOLE_RPC_PATHS.jsonRpc)) {
      const body = JSON.parse(String(init?.body || "{}"));
      if (body.method === CONSOLE_RPC_METHODS.capabilities) {
        return new Response(JSON.stringify({
          jsonrpc: "2.0",
          id: body.id,
          result: { methods: [CONSOLE_RPC_METHODS.send], version: "cap-v1" },
        }), { status: 200 });
      }
      if (body.method === CONSOLE_RPC_METHODS.send) {
        return new Response(JSON.stringify({
          jsonrpc: "2.0",
          id: body.id,
          result: { interaction_id: "turn-http", identity: body.params.identity },
        }), { status: 200 });
      }
    }
    return new Response("not found", { status: 404 });
  }) as typeof fetch;

  try {
    const transport = createHttpConsoleTransport({ baseUrl: "http://console.test" });
    assert.equal((await transport.loadExperience()).contract_version, "0.5.0");
    assert.deepEqual(await transport.capabilities(), {
      methods: [CONSOLE_RPC_METHODS.send],
      version: "cap-v1",
      runtime_capabilities: undefined,
      method_capabilities: undefined,
    });
    assert.equal((await transport.send({
      identity: "identity:lead",
      content: "hello",
      origin: "test",
      idempotencyKey: "idem-http",
    })).interaction_id, "turn-http");
    assert.deepEqual(calls.map((call) => [call.method, new URL(call.url).pathname]), [
      ["GET", CONSOLE_REST_PATHS.experience],
      ["POST", CONSOLE_RPC_PATHS.jsonRpc],
      ["POST", CONSOLE_RPC_PATHS.jsonRpc],
    ]);
  } finally {
    globalThis.fetch = previousFetch;
  }
});
