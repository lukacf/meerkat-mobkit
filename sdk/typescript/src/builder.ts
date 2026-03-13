/**
 * MobKit builder chain — chainable configuration for the runtime.
 *
 * @example
 * ```ts
 * import { MobKit } from "@rkat/mobkit-sdk";
 *
 * const rt = await MobKit.builder()
 *   .mob("config/mob.toml")
 *   .sessionService(builder, store)
 *   .discovery(discoverFn)
 *   .build();
 *
 * const handle = rt.mobHandle();
 * const status = await handle.status();
 * ```
 */

import { existsSync } from "node:fs";
import type { SessionAgentBuilder, ErrorCallback } from "./agent-builder.js";
import type { MobKitRuntime } from "./runtime.js";

// -- Builder config -------------------------------------------------------

export interface MobKitBuilderConfig {
  mobConfigPath: string | null;
  sessionBuilder: SessionAgentBuilder | null;
  sessionStore: unknown;
  discoveryCallback: unknown;
  preSpawnCallback: unknown;
  errorCallback: ErrorCallback | null;
  eventLog: Record<string, unknown> | null;
  gatingConfigPath: string | null;
  routingConfigPath: string | null;
  schedulingFiles: string[];
  memoryConfig: unknown;
  authConfig: unknown;
  gatewayBin: string | null;
  modules: unknown[];
}

function defaultConfig(): MobKitBuilderConfig {
  return {
    mobConfigPath: null,
    sessionBuilder: null,
    sessionStore: null,
    discoveryCallback: null,
    preSpawnCallback: null,
    errorCallback: null,
    eventLog: null,
    gatingConfigPath: null,
    routingConfigPath: null,
    schedulingFiles: [],
    memoryConfig: null,
    authConfig: null,
    gatewayBin: null,
    modules: [],
  };
}

// -- MobKitBuilder --------------------------------------------------------

/**
 * Chainable builder for MobKit runtime configuration.
 *
 * @example
 * ```ts
 * const rt = await MobKit.builder()
 *   .mob("config/mob.toml")
 *   .gateway("./target/release/phase0b_rpc_gateway")
 *   .build();
 * ```
 */
export class MobKitBuilder {
  /** @internal */
  readonly _config: MobKitBuilderConfig = defaultConfig();

  mob(configPath: string): this {
    this._config.mobConfigPath = configPath;
    return this;
  }

  sessionService(builder: SessionAgentBuilder, store?: unknown): this {
    this._config.sessionBuilder = builder;
    this._config.sessionStore = store ?? null;
    return this;
  }

  discovery(callback: unknown): this {
    this._config.discoveryCallback = callback;
    return this;
  }

  preSpawn(callback: unknown): this {
    this._config.preSpawnCallback = callback;
    return this;
  }

  eventLog(options: { storage: unknown;[key: string]: unknown }): this {
    this._config.eventLog = { ...options };
    return this;
  }

  onError(callback: ErrorCallback): this {
    this._config.errorCallback = callback;
    return this;
  }

  gating(configPath: string): this {
    this._config.gatingConfigPath = configPath;
    return this;
  }

  routing(configPath: string): this {
    this._config.routingConfigPath = configPath;
    return this;
  }

  scheduling(...scheduleFiles: string[]): this {
    this._config.schedulingFiles = scheduleFiles;
    return this;
  }

  memory(config?: unknown, options?: { stores?: string[] }): this {
    this._config.memoryConfig =
      config ?? { stores: options?.stores ?? [] };
    return this;
  }

  auth(config: unknown): this {
    this._config.authConfig = config;
    return this;
  }

  gateway(binPath: string): this {
    this._config.gatewayBin = binPath;
    return this;
  }

  modules(moduleSpecs: unknown[]): this {
    this._config.modules = moduleSpecs;
    return this;
  }

  async build(): Promise<MobKitRuntime> {
    this._applyConventionDefaults();
    // Dynamic import to break circular dep (runtime imports from builder config type)
    const { MobKitRuntime } = await import("./runtime.js");
    return MobKitRuntime._create(this._config);
  }

  private _applyConventionDefaults(): void {
    if (this._config.gatingConfigPath === null) {
      const candidate = "config/gating.toml";
      if (existsSync(candidate)) {
        this._config.gatingConfigPath = candidate;
      }
    }

    if (this._config.routingConfigPath === null) {
      const candidate = "deployment/routing.toml";
      if (existsSync(candidate)) {
        this._config.routingConfigPath = candidate;
      }
    }

    if (this._config.schedulingFiles.length === 0) {
      const files: string[] = [];
      const defaultFile = "config/defaults/schedules.toml";
      if (existsSync(defaultFile)) files.push(defaultFile);
      const overrideFile = "deployment/schedules.toml";
      if (existsSync(overrideFile)) files.push(overrideFile);
      if (files.length > 0) {
        this._config.schedulingFiles = files;
      }
    }
  }
}

// -- MobKit static factory ------------------------------------------------

export class MobKit {
  static builder(): MobKitBuilder {
    return new MobKitBuilder();
  }
}
