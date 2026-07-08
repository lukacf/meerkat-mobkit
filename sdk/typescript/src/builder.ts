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
import type {
  ContinuityStore,
  LeaseProvider,
  RosterProvider,
  AgentCustomizer,
  TopologyProvider,
} from "./types.js";

// -- agentMemory() key whitelists ------------------------------------------
// Mirror the gateway's supported-field lists (rpc_gateway.rs); the runtime
// checks exist so plain-JS callers fail loud on typos the way TS callers do
// at compile time.

const AGENT_MEMORY_KEYS = new Set([
  "enabled",
  "realm",
  "selection",
  "maxEntries",
  "recallTimeoutMs",
  "recallFailurePolicy",
  "instructionHeader",
  "perTurnInjection",
  "defangInbound",
  "store",
  "llmWrites",
  "recorderTool",
  "contentTrust",
  "selector",
  "operatorScope",
  "distiller",
  "steward",
  "hygienist",
]);

const CONTENT_TRUST_KEYS = new Set([
  "trustedMcpServers",
  "untrustedTools",
  "trustedTools",
]);

const DISTILLER_KEYS = new Set([
  "enabled",
  "runsPerHour",
  "minInteractions",
  "model",
]);

const STEWARD_KEYS = new Set([
  "enabled",
  "cadence",
  "model",
  "perMob",
  "runsPerDay",
  "minSignals",
]);

const HYGIENIST_KEYS = new Set(["enabled", "runsPerDay", "model"]);

function rejectUnknownKeys(
  config: object,
  known: Set<string>,
  context: string,
): void {
  const unknown = Object.keys(config).filter((key) => !known.has(key)).sort();
  if (unknown.length > 0) {
    throw new Error(
      `${context} got unsupported option(s): ${unknown.join(", ")}`,
    );
  }
}

// -- Builder config -------------------------------------------------------

export interface MobKitBuilderConfig {
  mobConfigPath: string | null;
  sessionBuilder: SessionAgentBuilder | null;
  sessionStore: unknown;
  discoveryCallback: unknown;
  preSpawnCallback: unknown;
  errorCallback: ErrorCallback | null;
  eventLog: Record<string, unknown> | null;
  consoleConfigPath: string | null;
  accessConfigPath: string | null;
  consoleRequireAppAuth: boolean | null;
  consoleReadOnly: boolean | null;
  consoleFetchTimeoutMs: number | null;
  demoLlm: boolean;
  gatingConfigPath: string | null;
  routingConfigPath: string | null;
  schedulingFiles: string[];
  workgraphEnabled: boolean | string | null;
  memoryConfig: unknown;
  agentMemoryConfig: unknown;
  authConfig: unknown;
  implicitDelegateIdleRetireSecs: number | null | undefined;
  maxSessions: number | null;
  gatewayTimeoutMs: number | null;
  gatewayBin: string | null;
  modules: unknown[];
  persistentState: string | null;
  continuityStore: ContinuityStore | null;
  leaseProvider: LeaseProvider | null;
  scratchDir: string | null;
  rosterProvider: RosterProvider | null;
  agentCustomizer: AgentCustomizer | null;
  topologyProvider: TopologyProvider | null;
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
    consoleConfigPath: null,
    accessConfigPath: null,
    consoleRequireAppAuth: null,
    consoleReadOnly: null,
    consoleFetchTimeoutMs: null,
    demoLlm: false,
    gatingConfigPath: null,
    routingConfigPath: null,
    schedulingFiles: [],
    workgraphEnabled: null,
    memoryConfig: null,
    agentMemoryConfig: null,
    authConfig: null,
    implicitDelegateIdleRetireSecs: undefined,
    maxSessions: null,
    gatewayTimeoutMs: null,
    gatewayBin: null,
    modules: [],
    persistentState: null,
    continuityStore: null,
    leaseProvider: null,
    scratchDir: null,
    rosterProvider: null,
    agentCustomizer: null,
    topologyProvider: null,
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
 *   .gateway("./target/release/mobkit_gateway")
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

  eventLog(options: { storage: unknown; [key: string]: unknown }): this {
    this._config.eventLog = { ...options };
    return this;
  }

  consoleConfig(configPath: string): this {
    this._config.consoleConfigPath = configPath;
    return this;
  }

  /**
   * Enable ABAC access control backed by a TOML file (conventionally
   * `config/access.toml`). A missing file starts disabled; console admin
   * edits persist back to the same path. Without this (and without a
   * conventional `config/access.toml`) access control is off entirely.
   */
  accessControl(configPath: string): this {
    this._config.accessConfigPath = configPath;
    return this;
  }

  consoleAuthRequired(required: boolean): this {
    this._config.consoleRequireAppAuth = required;
    return this;
  }

  consoleReadOnly(readOnly = true): this {
    this._config.consoleReadOnly = readOnly;
    return this;
  }

  consoleFetchTimeoutMs(timeoutMs: number): this {
    if (!Number.isInteger(timeoutMs) || timeoutMs <= 0) {
      throw new Error("consoleFetchTimeoutMs must be a positive integer");
    }
    this._config.consoleFetchTimeoutMs = timeoutMs;
    return this;
  }

  /**
   * Use the gateway's deterministic in-process LLM client.
   *
   * This is intended for local demos, smoke tests, and examples that need
   * autonomous agents to stay live without requiring provider credentials.
   */
  demoLlm(enabled = true): this {
    this._config.demoLlm = enabled;
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

  /**
   * Enable or disable WorkGraph service construction (goals, work items,
   * attention bindings). Defaults to enabled on the gateway; pass `false`
   * to opt out of construction entirely.
   */
  /**
   * Enable/disable WorkGraph, or pass a string DIRECTORY for an explicit
   * durable store location (`workgraph.sqlite3` created inside) — for
   * launches without a state dir that would otherwise be memory-backed.
   */
  workgraph(enabled: boolean | string = true): this {
    this._config.workgraphEnabled = enabled;
    return this;
  }

  memory(config?: unknown, options?: { stores?: string[] }): this {
    if (config === undefined && options?.stores !== undefined) {
      throw new Error(
        "memory(stores=...) is not supported by the Rust gateway; pass memory.localJson()",
      );
    }
    this._config.memoryConfig = config ?? null;
    return this;
  }

  agentMemory(
    config: true | {
      enabled?: boolean;
      realm?: string;
      selection?: "always" | "contextual";
      maxEntries?: number;
      recallTimeoutMs?: number;
      recallFailurePolicy?: "skip" | "fail";
      instructionHeader?: string;
      perTurnInjection?: "off" | "budgeted";
      defangInbound?: boolean;
      store?: "sqlite" | "markdown";
      llmWrites?: "observed" | "quarantined";
      recorderTool?: boolean;
      contentTrust?: {
        trustedMcpServers?: string[];
        untrustedTools?: string[];
        trustedTools?: string[];
      };
      selector?: "off" | "default" | `profile:${string}`;
      operatorScope?: "off" | "provisional";
      distiller?: boolean | {
        enabled?: boolean;
        runsPerHour?: number;
        minInteractions?: number;
        model?: string;
      };
      steward?: boolean | {
        enabled?: boolean;
        cadence?: string;
        model?: string;
        perMob?: boolean;
        runsPerDay?: number;
        minSignals?: number;
      };
      hygienist?: boolean | {
        enabled?: boolean;
        runsPerDay?: number;
        model?: string;
      };
    } = true,
  ): this {
    // Runtime unknown-key rejection for plain-JS callers (the type already
    // catches typos for TS callers): a typo'd option would otherwise be
    // silently dropped here, before the gateway's own fail-loud check.
    if (config !== true) {
      rejectUnknownKeys(config, AGENT_MEMORY_KEYS, "agentMemory");
    }
    if (config !== true && config.enabled === false) {
      this._config.agentMemoryConfig = { enabled: false };
      return this;
    }
    if (config === true) {
      this._config.agentMemoryConfig = true;
      return this;
    }
    const wire: Record<string, unknown> = {};
    if (config.enabled !== undefined) wire.enabled = config.enabled;
    if (config.realm !== undefined) wire.realm = config.realm;
    if (config.selection !== undefined) wire.selection = config.selection;
    if (config.maxEntries !== undefined) wire.max_entries = config.maxEntries;
    if (config.recallTimeoutMs !== undefined) {
      wire.recall_timeout_ms = config.recallTimeoutMs;
    }
    if (config.recallFailurePolicy !== undefined) {
      wire.recall_failure_policy = config.recallFailurePolicy;
    }
    if (config.instructionHeader !== undefined) {
      wire.instruction_header = config.instructionHeader;
    }
    if (config.perTurnInjection !== undefined) {
      wire.per_turn_injection = config.perTurnInjection;
    }
    if (config.defangInbound !== undefined) {
      wire.defang_inbound = config.defangInbound;
    }
    if (config.store !== undefined) wire.store = config.store;
    if (config.llmWrites !== undefined) wire.llm_writes = config.llmWrites;
    if (config.recorderTool !== undefined) {
      wire.recorder_tool = config.recorderTool;
    }
    if (config.contentTrust !== undefined) {
      rejectUnknownKeys(
        config.contentTrust,
        CONTENT_TRUST_KEYS,
        "agentMemory contentTrust",
      );
      const contentTrust: Record<string, unknown> = {};
      if (config.contentTrust.trustedMcpServers !== undefined) {
        contentTrust.trusted_mcp_servers = config.contentTrust.trustedMcpServers;
      }
      if (config.contentTrust.untrustedTools !== undefined) {
        contentTrust.untrusted_tools = config.contentTrust.untrustedTools;
      }
      if (config.contentTrust.trustedTools !== undefined) {
        contentTrust.trusted_tools = config.contentTrust.trustedTools;
      }
      wire.content_trust = contentTrust;
    }
    if (config.selector !== undefined) wire.selector = config.selector;
    if (config.operatorScope !== undefined) {
      wire.operator_scope = config.operatorScope;
    }
    if (config.distiller !== undefined) {
      if (typeof config.distiller === "boolean") {
        wire.distiller = config.distiller;
      } else {
        rejectUnknownKeys(
          config.distiller,
          DISTILLER_KEYS,
          "agentMemory distiller",
        );
        const distiller: Record<string, unknown> = {};
        if (config.distiller.enabled !== undefined) {
          distiller.enabled = config.distiller.enabled;
        }
        if (config.distiller.runsPerHour !== undefined) {
          distiller.runs_per_hour = config.distiller.runsPerHour;
        }
        if (config.distiller.minInteractions !== undefined) {
          distiller.min_interactions = config.distiller.minInteractions;
        }
        if (config.distiller.model !== undefined) {
          distiller.model = config.distiller.model;
        }
        wire.distiller = distiller;
      }
    }
    if (config.steward !== undefined) {
      if (typeof config.steward === "boolean") {
        wire.steward = config.steward;
      } else {
        rejectUnknownKeys(config.steward, STEWARD_KEYS, "agentMemory steward");
        const steward: Record<string, unknown> = {};
        if (config.steward.enabled !== undefined) {
          steward.enabled = config.steward.enabled;
        }
        if (config.steward.cadence !== undefined) {
          steward.cadence = config.steward.cadence;
        }
        if (config.steward.model !== undefined) {
          steward.model = config.steward.model;
        }
        if (config.steward.perMob !== undefined) {
          steward.per_mob = config.steward.perMob;
        }
        if (config.steward.runsPerDay !== undefined) {
          steward.runs_per_day = config.steward.runsPerDay;
        }
        if (config.steward.minSignals !== undefined) {
          steward.min_signals = config.steward.minSignals;
        }
        wire.steward = steward;
      }
    }
    if (config.hygienist !== undefined) {
      if (typeof config.hygienist === "boolean") {
        wire.hygienist = config.hygienist;
      } else {
        rejectUnknownKeys(
          config.hygienist,
          HYGIENIST_KEYS,
          "agentMemory hygienist",
        );
        const hygienist: Record<string, unknown> = {};
        if (config.hygienist.enabled !== undefined) {
          hygienist.enabled = config.hygienist.enabled;
        }
        if (config.hygienist.runsPerDay !== undefined) {
          hygienist.runs_per_day = config.hygienist.runsPerDay;
        }
        if (config.hygienist.model !== undefined) {
          hygienist.model = config.hygienist.model;
        }
        wire.hygienist = hygienist;
      }
    }
    this._config.agentMemoryConfig = wire;
    return this;
  }

  auth(config: unknown): this {
    this._config.authConfig = config;
    return this;
  }

  implicitDelegateIdleRetirement(seconds: number | null): this {
    if (seconds !== null && (!Number.isFinite(seconds) || seconds < 0)) {
      throw new Error(
        "implicit delegate idle retirement seconds must be non-negative or null",
      );
    }
    this._config.implicitDelegateIdleRetireSecs = seconds;
    return this;
  }

  maxSessions(maxSessions: number): this {
    if (!Number.isInteger(maxSessions) || maxSessions <= 0) {
      throw new Error("maxSessions must be a positive integer");
    }
    this._config.maxSessions = maxSessions;
    return this;
  }

  gatewayTimeoutMs(timeoutMs: number): this {
    if (!Number.isInteger(timeoutMs) || timeoutMs <= 0) {
      throw new Error("gatewayTimeoutMs must be a positive integer");
    }
    this._config.gatewayTimeoutMs = timeoutMs;
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

  /**
   * Enable persistent state at the given path.
   *
   * When set, the gateway creates SQLite-backed session/runtime state,
   * MobKit metadata, console logs, and binary blob storage under this
   * directory. Mob storage remains in-memory.
   */
  persistentState(path: string): this {
    this._config.persistentState = path;
    return this;
  }

  /** Set an external ContinuityStore provider. Mutually exclusive with persistentState. */
  continuityStore(store: ContinuityStore): this {
    this._config.continuityStore = store;
    return this;
  }

  /** Set an external LeaseProvider. Mutually exclusive with persistentState. */
  leaseProvider(provider: LeaseProvider): this {
    this._config.leaseProvider = provider;
    return this;
  }

  /** Set scratch directory for external provider path. */
  scratchDir(path: string): this {
    this._config.scratchDir = path;
    return this;
  }

  /** Set a RosterProvider. */
  rosterProvider(provider: RosterProvider): this {
    this._config.rosterProvider = provider;
    return this;
  }

  /** Set an AgentCustomizer. */
  agentCustomizer(customizer: AgentCustomizer): this {
    this._config.agentCustomizer = customizer;
    return this;
  }

  /** Set a TopologyProvider. */
  topologyProvider(provider: TopologyProvider): this {
    this._config.topologyProvider = provider;
    return this;
  }

  async build(): Promise<MobKitRuntime> {
    this._validateConfig();
    this._applyConventionDefaults();
    // Dynamic import to break circular dep (runtime imports from builder config type)
    const { MobKitRuntime } = await import("./runtime.js");
    return MobKitRuntime._create(this._config);
  }

  private _validateConfig(): void {
    const hasPersistent = this._config.persistentState !== null;
    const hasExternal =
      this._config.continuityStore !== null ||
      this._config.leaseProvider !== null ||
      this._config.scratchDir !== null;
    if (hasPersistent && hasExternal) {
      throw new Error(
        "persistentState and continuityStore/leaseProvider are mutually exclusive — " +
          "use one path or the other, not both",
      );
    }
    if (hasExternal) {
      const missing: string[] = [];
      if (this._config.continuityStore === null)
        missing.push("continuityStore");
      if (this._config.leaseProvider === null) missing.push("leaseProvider");
      if (this._config.scratchDir === null) missing.push("scratchDir");
      if (missing.length > 0) {
        throw new Error(
          "external-authoritative path requires continuityStore() + leaseProvider() + " +
            `scratchDir(); missing: ${missing.join(", ")}`,
        );
      }
    }
  }

  private _applyConventionDefaults(): void {
    if (this._config.consoleConfigPath === null) {
      const candidate = "config/console.toml";
      if (existsSync(candidate)) {
        this._config.consoleConfigPath = candidate;
      }
    }

    if (this._config.accessConfigPath === null) {
      const candidate = "config/access.toml";
      if (existsSync(candidate)) {
        this._config.accessConfigPath = candidate;
      }
    }

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
