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

  it("serializes the selector switch to the gateway wire key", () => {
    const builder = MobKit.builder();
    builder.agentMemory({ selector: "profile:/etc/mobkit/selector.toml" });

    assert.deepEqual(builder._config.agentMemoryConfig, {
      selector: "profile:/etc/mobkit/selector.toml",
    });
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
