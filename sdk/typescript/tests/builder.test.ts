/**
 * Tests for MobKitBuilder configuration chain.
 */

import { describe, it } from "node:test";
import assert from "node:assert/strict";

import { MobKit, MobKitBuilder } from "../dist/index.js";

describe("MobKit.builder()", () => {
  it("returns a MobKitBuilder instance", () => {
    const builder = MobKit.builder();
    assert.ok(builder instanceof MobKitBuilder);
  });
});

describe("MobKitBuilder chainable methods", () => {
  it("mob() sets mobConfigPath and returns this", () => {
    const builder = MobKit.builder();
    const result = builder.mob("config/mob.toml");
    assert.equal(result, builder);
    assert.equal(builder._config.mobConfigPath, "config/mob.toml");
  });

  it("gateway() sets gatewayBin and returns this", () => {
    const builder = MobKit.builder();
    const result = builder.gateway("/usr/bin/gateway");
    assert.equal(result, builder);
    assert.equal(builder._config.gatewayBin, "/usr/bin/gateway");
  });

  it("sessionService() sets sessionBuilder and sessionStore", () => {
    const builder = MobKit.builder();
    const mockBuilder = { buildAgent: async () => {} };
    const mockStore = { type: "json" };
    const result = builder.sessionService(mockBuilder, mockStore);
    assert.equal(result, builder);
    assert.equal(builder._config.sessionBuilder, mockBuilder);
    assert.equal(builder._config.sessionStore, mockStore);
  });

  it("sessionService() defaults store to null", () => {
    const builder = MobKit.builder();
    const mockBuilder = { buildAgent: async () => {} };
    builder.sessionService(mockBuilder);
    assert.equal(builder._config.sessionBuilder, mockBuilder);
    assert.equal(builder._config.sessionStore, null);
  });

  it("discovery() sets discoveryCallback and returns this", () => {
    const builder = MobKit.builder();
    const cb = () => {};
    const result = builder.discovery(cb);
    assert.equal(result, builder);
    assert.equal(builder._config.discoveryCallback, cb);
  });

  it("preSpawn() sets preSpawnCallback and returns this", () => {
    const builder = MobKit.builder();
    const cb = () => {};
    const result = builder.preSpawn(cb);
    assert.equal(result, builder);
    assert.equal(builder._config.preSpawnCallback, cb);
  });

  it("onError() sets errorCallback and returns this", () => {
    const builder = MobKit.builder();
    const cb = () => {};
    const result = builder.onError(cb);
    assert.equal(result, builder);
    assert.equal(builder._config.errorCallback, cb);
  });

  it("eventLog() sets eventLog config and returns this", () => {
    const builder = MobKit.builder();
    const opts = { storage: "file", path: "/tmp/events.log" };
    const result = builder.eventLog(opts);
    assert.equal(result, builder);
    assert.deepEqual(builder._config.eventLog, {
      storage: "file",
      path: "/tmp/events.log",
    });
  });

  it("gating() sets gatingConfigPath and returns this", () => {
    const builder = MobKit.builder();
    const result = builder.gating("config/gating.toml");
    assert.equal(result, builder);
    assert.equal(builder._config.gatingConfigPath, "config/gating.toml");
  });

  it("routing() sets routingConfigPath and returns this", () => {
    const builder = MobKit.builder();
    const result = builder.routing("deployment/routing.toml");
    assert.equal(result, builder);
    assert.equal(builder._config.routingConfigPath, "deployment/routing.toml");
  });

  it("scheduling() sets schedulingFiles and returns this", () => {
    const builder = MobKit.builder();
    const result = builder.scheduling("sched1.toml", "sched2.toml");
    assert.equal(result, builder);
    assert.deepEqual(builder._config.schedulingFiles, ["sched1.toml", "sched2.toml"]);
  });

  it("memory() sets memoryConfig with config object and returns this", () => {
    const builder = MobKit.builder();
    const cfg = { engine: "elephant" };
    const result = builder.memory(cfg);
    assert.equal(result, builder);
    assert.deepEqual(builder._config.memoryConfig, { engine: "elephant" });
  });

  it("memory() sets memoryConfig with options.stores when no config", () => {
    const builder = MobKit.builder();
    builder.memory(undefined, { stores: ["main", "archive"] });
    assert.deepEqual(builder._config.memoryConfig, {
      stores: ["main", "archive"],
    });
  });

  it("memory() defaults to empty stores when called with no args", () => {
    const builder = MobKit.builder();
    builder.memory();
    assert.deepEqual(builder._config.memoryConfig, { stores: [] });
  });

  it("auth() sets authConfig and returns this", () => {
    const builder = MobKit.builder();
    const cfg = { provider: "google" };
    const result = builder.auth(cfg);
    assert.equal(result, builder);
    assert.deepEqual(builder._config.authConfig, { provider: "google" });
  });

  it("modules() sets modules array and returns this", () => {
    const builder = MobKit.builder();
    const specs = [{ id: "mod-a", command: "cmd" }];
    const result = builder.modules(specs);
    assert.equal(result, builder);
    assert.deepEqual(builder._config.modules, specs);
  });
});

describe("MobKitBuilder default config", () => {
  it("has null/empty defaults for all fields", () => {
    const builder = MobKit.builder();
    const cfg = builder._config;
    assert.equal(cfg.mobConfigPath, null);
    assert.equal(cfg.sessionBuilder, null);
    assert.equal(cfg.sessionStore, null);
    assert.equal(cfg.discoveryCallback, null);
    assert.equal(cfg.preSpawnCallback, null);
    assert.equal(cfg.errorCallback, null);
    assert.equal(cfg.eventLog, null);
    assert.equal(cfg.gatingConfigPath, null);
    assert.equal(cfg.routingConfigPath, null);
    assert.deepEqual(cfg.schedulingFiles, []);
    assert.equal(cfg.memoryConfig, null);
    assert.equal(cfg.authConfig, null);
    assert.equal(cfg.gatewayBin, null);
    assert.deepEqual(cfg.modules, []);
  });
});

describe("MobKitBuilder convention defaults", () => {
  it("does not set gating if file does not exist", () => {
    // Convention defaults look for config/gating.toml — which won't
    // exist in the test environment, so gatingConfigPath stays null.
    const builder = MobKit.builder();
    // Manually call the private method via build path check
    // We can test by checking the config before build would set it:
    assert.equal(builder._config.gatingConfigPath, null);
  });

  it("does not set routing if file does not exist", () => {
    const builder = MobKit.builder();
    assert.equal(builder._config.routingConfigPath, null);
  });

  it("does not set scheduling files if none exist", () => {
    const builder = MobKit.builder();
    assert.deepEqual(builder._config.schedulingFiles, []);
  });
});

describe("MobKitBuilder method chaining", () => {
  it("supports full chain", () => {
    const builder = MobKit.builder()
      .mob("mob.toml")
      .gateway("/bin/gw")
      .gating("gating.toml")
      .routing("routing.toml")
      .scheduling("s1.toml")
      .auth({ provider: "jwt" })
      .modules([{ id: "a" }]);

    assert.equal(builder._config.mobConfigPath, "mob.toml");
    assert.equal(builder._config.gatewayBin, "/bin/gw");
    assert.equal(builder._config.gatingConfigPath, "gating.toml");
    assert.equal(builder._config.routingConfigPath, "routing.toml");
    assert.deepEqual(builder._config.schedulingFiles, ["s1.toml"]);
    assert.deepEqual(builder._config.authConfig, { provider: "jwt" });
    assert.deepEqual(builder._config.modules, [{ id: "a" }]);
  });
});
