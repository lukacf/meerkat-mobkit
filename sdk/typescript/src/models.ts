/**
 * Typed data models for MobKit SDK — input/config objects sent to the runtime.
 */

import { DetachedJobExecution } from "./jobs.js";

// -- DiscoverySpec --------------------------------------------------------

export interface DiscoverySpec {
  readonly role: string;
  readonly agentIdentity: string;
  readonly labels?: Readonly<Record<string, string>>;
  readonly appContext?: unknown;
  readonly additionalInstructions?: readonly string[];
  readonly resumeSessionId?: string;
}

export function discoverySpecToDict(
  spec: DiscoverySpec,
): Record<string, unknown> {
  const result: Record<string, unknown> = {
    role: spec.role,
    agent_identity: spec.agentIdentity,
  };
  if (spec.labels && Object.keys(spec.labels).length > 0) {
    result.labels = { ...spec.labels };
  }
  if (spec.appContext !== undefined) {
    result.app_context = spec.appContext;
  }
  if (spec.additionalInstructions && spec.additionalInstructions.length > 0) {
    result.additional_instructions = [...spec.additionalInstructions];
  }
  if (spec.resumeSessionId !== undefined) {
    result.resume_session_id = spec.resumeSessionId;
  }
  return result;
}

// -- PreSpawnData ---------------------------------------------------------

export interface PreSpawnData {
  readonly resumeMap?: Readonly<Record<string, string>>;
  readonly moduleId?: string;
  readonly env?: Readonly<Record<string, string>>;
}

export function preSpawnDataToDict(
  data: PreSpawnData,
): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  if (data.resumeMap && Object.keys(data.resumeMap).length > 0) {
    result.resume_map = { ...data.resumeMap };
  }
  if (data.moduleId !== undefined) {
    result.module_id = data.moduleId;
  }
  if (data.env && Object.keys(data.env).length > 0) {
    result.env = Object.entries(data.env);
  }
  return result;
}

// -- SessionQuery ---------------------------------------------------------

export interface SessionQuery {
  readonly agentType?: string;
  readonly ownerId?: string;
  readonly labels?: Readonly<Record<string, string>>;
  readonly includeDeleted?: boolean;
  readonly limit?: number;
}

export function sessionQueryToDict(
  query: SessionQuery,
): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  if (query.agentType !== undefined) result.agent_type = query.agentType;
  if (query.ownerId !== undefined) result.owner_id = query.ownerId;
  if (query.labels && Object.keys(query.labels).length > 0) {
    result.labels = { ...query.labels };
  }
  result.include_deleted = query.includeDeleted ?? false;
  result.limit = query.limit ?? 100;
  return result;
}

// -- Fork lineage -----------------------------------------------------------

/**
 * A mob member as meerkat names it: its mob, role and roster member id.
 *
 * In a MobKit mob `member` is MobKit's comms-safe roster encoding of the
 * member's durable identity (`mk--...`), not the identity itself.
 */
export interface MobMemberBinding {
  readonly mobId: string;
  readonly role: string;
  readonly member: string;
}

/**
 * The source a fork-derived member was forked from (meerkat 0.8.45+).
 *
 * Set by the mob runtime on the build that seats a durable fork (fork_off
 * children, fork_member children, local council participants) and on every
 * later rebuild of that member. Absent for every other build.
 */
export interface ForkBuildSource {
  readonly sourceMember: MobMemberBinding;
  readonly sourceSessionId: string;
}

function requiredString(
  data: Record<string, unknown>,
  key: string,
  owner: string,
): string {
  const value = data[key];
  if (typeof value !== "string") {
    throw new TypeError(`${owner}.${key} must be a string, got ${typeof value}`);
  }
  return value;
}

function asObject(raw: unknown, owner: string): Record<string, unknown> {
  if (typeof raw !== "object" || raw === null || Array.isArray(raw)) {
    throw new TypeError(`${owner} must be an object`);
  }
  return raw as Record<string, unknown>;
}

/**
 * Decode the gateway's `fork_source` object. `null`/absent decodes to
 * `null`; fields this SDK does not know are ignored, so a newer gateway can
 * add fields without breaking older builders.
 */
export function parseForkBuildSource(raw: unknown): ForkBuildSource | null {
  if (raw === undefined || raw === null) return null;
  const data = asObject(raw, "ForkBuildSource");
  const member = asObject(data.source_member, "ForkBuildSource.source_member");
  return {
    sourceMember: {
      mobId: requiredString(member, "mob_id", "MobMemberBinding"),
      role: requiredString(member, "role", "MobMemberBinding"),
      member: requiredString(member, "member", "MobMemberBinding"),
    },
    sourceSessionId: requiredString(data, "source_session_id", "ForkBuildSource"),
  };
}

// -- SessionBuildOptions --------------------------------------------------

/** Callback tool handler: receives arguments dict, returns JSON-serializable result. */
export type ToolHandler = (
  args: Record<string, unknown>,
) => unknown | Promise<unknown>;

/**
 * Mutable options passed to {@link SessionAgentBuilder.buildAgent}.
 *
 * The builder mutates fields during agent construction — sets profileName,
 * calls {@link addTools} or {@link registerTool}.
 *
 * @example
 * ```ts
 * const builder: SessionAgentBuilder = {
 *   async buildAgent(opts) {
 *     opts.profileName = "assistant";
 *     opts.registerTool("search", searchHandler);
 *   },
 * };
 * ```
 */
export class SessionBuildOptions {
  appContext: unknown = undefined;
  additionalInstructions: string[] = [];
  sessionId: string | null = null;
  labels: Record<string, string> = {};
  profileName: string | null = null;
  /**
   * The session this build resumes, when the gateway resumes one; a builder
   * may also set it to ask the gateway to resume that session (persistent
   * mode only).
   */
  resumeSessionId: string | null = null;
  /**
   * Receive-only fork lineage: which member this member was forked from, so
   * the builder can build the child as its source. Never sent back, and the
   * gateway ignores it in a build response: fork lineage is set by the mob
   * runtime alone. `forkSource.sourceMember.member` is MobKit's encoded
   * roster id; {@link forkSourceIdentity} is the source's durable identity
   * (for example `domain:calendar`), the key to resolve grants by. The
   * child's own `labels` and `sessionId` still name the child.
   */
  readonly forkSource: ForkBuildSource | null;
  /** The fork source's durable identity; see {@link forkSource}. */
  readonly forkSourceIdentity: string | null;

  private _tools: string[] = [];
  private _toolHandlers: Map<string, ToolHandler> = new Map();
  private _jobExecutions: Map<string, DetachedJobExecution> = new Map();
  // Per-tool wire metadata from registerTool options; tools without an
  // entry cross the wire as bare name strings.
  private _toolDefs: Map<string, Record<string, unknown>> = new Map();

  constructor(init?: {
    forkSource?: ForkBuildSource | null;
    forkSourceIdentity?: string | null;
  }) {
    this.forkSource = init?.forkSource ?? null;
    this.forkSourceIdentity = init?.forkSourceIdentity ?? null;
  }

  /** Declare tool names the agent can use. */
  addTools(tools: string[]): void {
    for (const t of tools) {
      if (typeof t !== "string") {
        throw new TypeError(
          `tools must be strings, got ${typeof t}: ${String(t)}`,
        );
      }
    }
    this._tools.push(...tools);
  }

  /**
   * Register a callable tool with the agent.
   *
   * `options.inputSchema` is the JSON Schema for the tool arguments; when
   * omitted the gateway advertises the permissive `{"type": "object"}`.
   */
  registerTool(
    name: string,
    handler: ToolHandler,
    options?: {
      description?: string;
      inputSchema?: Record<string, unknown>;
      execution?: DetachedJobExecution;
    },
  ): void {
    if (typeof name !== "string") {
      throw new TypeError(
        `tool name must be a string, got ${typeof name}: ${String(name)}`,
      );
    }
    if (typeof handler !== "function") {
      throw new TypeError(
        `handler must be callable, got ${typeof handler}: ${String(handler)}`,
      );
    }
    this._tools.push(name);
    this._toolHandlers.set(name, handler);
    if (
      options?.execution !== undefined &&
      !(options.execution instanceof DetachedJobExecution)
    ) {
      throw new TypeError("execution must be a DetachedJobExecution");
    }
    if (
      options?.description !== undefined ||
      options?.inputSchema !== undefined ||
      options?.execution !== undefined
    ) {
      const def: Record<string, unknown> = { name };
      if (options.description !== undefined) def.description = options.description;
      if (options.inputSchema !== undefined) def.input_schema = options.inputSchema;
      if (options.execution !== undefined) {
        def.execution = options.execution.toWire();
        this._jobExecutions.set(name, options.execution);
      }
      this._toolDefs.set(name, def);
    }
  }

  get tools(): string[] {
    return [...this._tools];
  }

  get toolHandlers(): ReadonlyMap<string, ToolHandler> {
    return new Map(this._toolHandlers);
  }

  get jobExecutions(): ReadonlyMap<string, DetachedJobExecution> {
    return new Map(this._jobExecutions);
  }

  toDict(): Record<string, unknown> {
    const result: Record<string, unknown> = {};
    if (this.appContext !== undefined) result.app_context = this.appContext;
    if (this.additionalInstructions.length > 0) {
      result.additional_instructions = [...this.additionalInstructions];
    }
    if (this.sessionId !== null) result.session_id = this.sessionId;
    if (Object.keys(this.labels).length > 0) {
      result.labels = { ...this.labels };
    }
    if (this.profileName !== null) result.profile_name = this.profileName;
    if (this.resumeSessionId !== null) {
      result.resume_session_id = this.resumeSessionId;
    }
    if (this._tools.length > 0) {
      // Names with registered metadata cross as {name, description?,
      // input_schema?} objects; everything else stays a bare string
      // (backward-compatible with pre-0.7.30 gateways).
      result.tools = this._tools.map(
        (name) => this._toolDefs.get(name) ?? name,
      );
    }
    return result;
  }
}
