/**
 * TDD tests for the new builder API: persistentState, afterCreate, SessionCreatedContext.
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { MobKit, MobKitBuilder } from "../dist/index.js";
import { CallbackDispatcher } from "../dist/agent-builder.js";
import { SessionBuildOptions } from "../dist/models.js";
import type { SessionCreatedContext } from "../dist/types.js";

// ---------------------------------------------------------------------------
// persistentState on builder
// ---------------------------------------------------------------------------

describe("MobKitBuilder.persistentState()", () => {
  it("returns this for chaining", () => {
    const builder = MobKit.builder();
    const result = builder.persistentState("/tmp/test-state");
    assert.equal(result, builder);
  });

  it("sets persistentState on config", () => {
    const builder = MobKit.builder();
    builder.persistentState("/tmp/test-state");
    assert.equal(builder._config.persistentState, "/tmp/test-state");
  });

  it("defaults to null", () => {
    const builder = MobKit.builder();
    assert.equal(builder._config.persistentState, null);
  });
});

describe("MobKitBuilder.agentMemory()", () => {
  it("defaults to disabled", () => {
    const builder = MobKit.builder();
    assert.equal(builder._config.agentMemoryConfig, null);
  });

  it("stores true for default gateway configuration", () => {
    const builder = MobKit.builder();
    const result = builder.agentMemory();

    assert.equal(result, builder);
    assert.equal(builder._config.agentMemoryConfig, true);
  });

  it("serializes camelCase options to gateway wire keys", () => {
    const builder = MobKit.builder();
    builder.agentMemory({
      realm: "family",
      selection: "contextual",
      maxEntries: 3,
      recallTimeoutMs: 1200,
      recallFailurePolicy: "fail",
      instructionHeader: "Remember",
    });

    assert.deepEqual(builder._config.agentMemoryConfig, {
      realm: "family",
      selection: "contextual",
      max_entries: 3,
      recall_timeout_ms: 1200,
      recall_failure_policy: "fail",
      instruction_header: "Remember",
    });
  });

  it("serializes taint knobs to gateway wire keys", () => {
    const builder = MobKit.builder();
    builder.agentMemory({
      llmWrites: "quarantined",
      recorderTool: false,
      contentTrust: {
        trustedMcpServers: ["knowledge_graph"],
        untrustedTools: ["scrape_page"],
        trustedTools: ["safe_calc"],
      },
    });

    assert.deepEqual(builder._config.agentMemoryConfig, {
      llm_writes: "quarantined",
      recorder_tool: false,
      content_trust: {
        trusted_mcp_servers: ["knowledge_graph"],
        untrusted_tools: ["scrape_page"],
        trusted_tools: ["safe_calc"],
      },
    });
  });

  it("keeps only the retired selector off compatibility form", () => {
    const builder = MobKit.builder();
    builder.agentMemory({ selector: "off" });

    assert.deepEqual(builder._config.agentMemoryConfig, {
      selector: "off",
    });
    assert.throws(
      () => builder.agentMemory({ selector: "profile:/tmp/selector.toml" } as never),
      /selector is RETIRED/,
    );
  });

  it("serializes the distiller block to gateway wire keys", () => {
    const builder = MobKit.builder();
    builder.agentMemory({
      distiller: {
        enabled: true,
        runsPerHour: 6,
        minInteractions: 5,
        model: "claude-haiku-4-5",
      },
    });

    assert.deepEqual(builder._config.agentMemoryConfig, {
      distiller: {
        enabled: true,
        runs_per_hour: 6,
        min_interactions: 5,
        model: "claude-haiku-4-5",
      },
    });

    const boolBuilder = MobKit.builder();
    boolBuilder.agentMemory({ distiller: true });
    assert.deepEqual(boolBuilder._config.agentMemoryConfig, { distiller: true });
  });

  it("serializes the steward block to gateway wire keys", () => {
    const builder = MobKit.builder();
    builder.agentMemory({
      steward: {
        enabled: true,
        cadence: "*/6h",
        model: "claude-sonnet-4-6",
        perMob: false,
        runsPerDay: 4,
        minSignals: 3,
      },
    });

    assert.deepEqual(builder._config.agentMemoryConfig, {
      steward: {
        enabled: true,
        cadence: "*/6h",
        model: "claude-sonnet-4-6",
        per_mob: false,
        runs_per_day: 4,
        min_signals: 3,
      },
    });

    const boolBuilder = MobKit.builder();
    boolBuilder.agentMemory({ steward: true });
    assert.deepEqual(boolBuilder._config.agentMemoryConfig, { steward: true });
  });

  it("serializes operatorScope to the gateway wire key", () => {
    const builder = MobKit.builder();
    builder.agentMemory({ store: "sqlite", operatorScope: "provisional" });

    assert.deepEqual(builder._config.agentMemoryConfig, {
      store: "sqlite",
      operator_scope: "provisional",
    });
  });

  it("keeps only disabled hygienist compatibility forms", () => {
    const builder = MobKit.builder();
    builder.agentMemory({
      hygienist: {
        enabled: false,
        runsPerDay: 3,
        model: "legacy-model",
        maxOutputTokens: 8192,
      },
    });

    assert.deepEqual(builder._config.agentMemoryConfig, {
      hygienist: {
        enabled: false,
        runs_per_day: 3,
        model: "legacy-model",
        max_output_tokens: 8192,
      },
    });

    const boolBuilder = MobKit.builder();
    boolBuilder.agentMemory({ hygienist: false });
    assert.deepEqual(boolBuilder._config.agentMemoryConfig, { hygienist: false });

    assert.throws(
      () => MobKit.builder().agentMemory({ hygienist: true } as never),
      /hygienist is PARKED and cannot be enabled/,
    );
    assert.throws(
      () => MobKit.builder().agentMemory({ hygienist: {} } as never),
      /hygienist is PARKED and cannot be enabled/,
    );
  });

  it("rejects unknown options at runtime instead of silently dropping them", () => {
    const builder = MobKit.builder();
    assert.throws(
      // Cast simulates a plain-JS caller; the TS type already rejects this.
      () => builder.agentMemory({ perTurnInjecton: "budgeted" } as never),
      /agentMemory got unsupported option\(s\): perTurnInjecton/,
    );
  });

  it("rejects unknown nested options at runtime", () => {
    assert.throws(
      () =>
        MobKit.builder().agentMemory({
          distiller: { runsPerHour: 2, runsperhourTypo: 9 },
        } as never),
      /agentMemory distiller got unsupported option\(s\): runsperhourTypo/,
    );
    assert.throws(
      () => MobKit.builder().agentMemory({ steward: { cadance: "*/6h" } } as never),
      /agentMemory steward got unsupported option\(s\): cadance/,
    );
    assert.throws(
      () =>
        MobKit.builder().agentMemory({
          hygienist: { enabled: false, runsPerDya: 2 },
        } as never),
      /agentMemory hygienist got unsupported option\(s\): runsPerDya/,
    );
    assert.throws(
      () =>
        MobKit.builder().agentMemory({
          contentTrust: { trustedMcpServrs: [] },
        } as never),
      /agentMemory contentTrust got unsupported option\(s\): trustedMcpServrs/,
    );
  });
});

// ---------------------------------------------------------------------------
// callback/after_create dispatch
// ---------------------------------------------------------------------------

describe("CallbackDispatcher callback/after_create", () => {
  it("routes to builder.afterCreate()", async () => {
    const received: { sessionId?: string; context?: SessionCreatedContext } = {};

    const dispatcher = new CallbackDispatcher();
    dispatcher.registerBuilder({
      async buildAgent(_opts: SessionBuildOptions) {},
      async afterCreate(sessionId: string, context: SessionCreatedContext) {
        received.sessionId = sessionId;
        received.context = context;
      },
    });

    await dispatcher.handleCallback("callback/after_create", {
      session_id: "sid-123",
      model: "claude-sonnet-4-5",
      labels: { agent_type: "lead" },
      system_prompt: "You are a lead.",
    });

    assert.equal(received.sessionId, "sid-123");
    assert.equal(received.context?.model, "claude-sonnet-4-5");
    assert.deepEqual(received.context?.labels, { agent_type: "lead" });
    assert.equal(received.context?.systemPrompt, "You are a lead.");
  });

  it("is a no-op when builder has no afterCreate", async () => {
    const dispatcher = new CallbackDispatcher();
    dispatcher.registerBuilder({
      async buildAgent(_opts: SessionBuildOptions) {},
    });

    // Should not throw.
    await dispatcher.handleCallback("callback/after_create", {
      session_id: "sid-456",
      model: "test-model",
      labels: {},
      system_prompt: null,
    });
  });

  it("swallows afterCreate errors (best-effort)", async () => {
    const dispatcher = new CallbackDispatcher();
    dispatcher.registerBuilder({
      async buildAgent(_opts: SessionBuildOptions) {},
      async afterCreate(_sessionId: string, _context: SessionCreatedContext) {
        throw new Error("db unavailable");
      },
    });

    // Should not throw.
    await dispatcher.handleCallback("callback/after_create", {
      session_id: "sid-789",
      model: "test-model",
      labels: {},
      system_prompt: null,
    });
  });
});

// ---------------------------------------------------------------------------
// SessionCreatedContext interface
// ---------------------------------------------------------------------------

describe("SessionCreatedContext", () => {
  it("can be constructed from wire format", () => {
    const ctx: SessionCreatedContext = {
      model: "claude-sonnet-4-5",
      labels: { agent_type: "lead" },
      systemPrompt: "You are a lead agent.",
    };
    assert.equal(ctx.model, "claude-sonnet-4-5");
    assert.deepEqual(ctx.labels, { agent_type: "lead" });
    assert.equal(ctx.systemPrompt, "You are a lead agent.");
  });
});
