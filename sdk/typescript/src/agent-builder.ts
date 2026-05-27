/**
 * SessionAgentBuilder protocol and CallbackDispatcher.
 *
 * The builder protocol uses imperative mutation: buildAgent receives a
 * mutable SessionBuildOptions and modifies it in place.
 */

import { SessionBuildOptions, type ToolHandler } from "./models.js";
import {
  parseErrorEvent,
  parseContinuityRecord,
  parseSessionSnapshot,
  parseLeaseGrant,
  parseAgentBuildContext,
  parseDurableAgentSpec,
  parseAgentBuildDraft,
  agentBuildDraftToDict,
  durableAgentSpecToDict,
  sessionSnapshotToDict,
  leaseGrantToDict,
  leaseAcquireResultToDict,
  leaseRenewResultToDict,
  managedPeerEdgeToDict,
  type ErrorEvent,
  type SessionCreatedContext,
  type ContinuityStore,
  type LeaseProvider,
  type RosterProvider,
  type AgentCustomizer,
  type TopologyProvider,
  type LeaseGrant,
} from "./types.js";

// -- Protocol -------------------------------------------------------------

/**
 * Protocol for building agents during session creation.
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
export interface SessionAgentBuilder {
  buildAgent(options: SessionBuildOptions): Promise<void>;
  /** Called after session creation succeeds. Best-effort — errors logged, not propagated. */
  afterCreate?(sessionId: string, context: SessionCreatedContext): Promise<void>;
}

// -- Error callback type --------------------------------------------------

export type ErrorCallback = (event: ErrorEvent) => void | Promise<void>;

// -- CallbackDispatcher ---------------------------------------------------

/**
 * Routes incoming JSON-RPC callbacks from the Rust runtime to the
 * registered SessionAgentBuilder and tool handlers.
 *
 * Tool handlers are scoped by a build-level scope_id to prevent
 * cross-session handler bleed in concurrent sessions.
 */
export class CallbackDispatcher {
  private _builder: SessionAgentBuilder | null = null;
  private _errorCallback: ErrorCallback | null = null;
  private readonly _toolHandlers = new Map<string, ToolHandler>();
  private readonly _scopeTools = new Map<string, string[]>();
  private _continuityStore: ContinuityStore | null = null;
  private _leaseProvider: LeaseProvider | null = null;
  private _rosterProvider: RosterProvider | null = null;
  private _agentCustomizer: AgentCustomizer | null = null;
  private _topologyProvider: TopologyProvider | null = null;

  registerBuilder(builder: SessionAgentBuilder): void {
    this._builder = builder;
  }

  registerErrorCallback(callback: ErrorCallback): void {
    this._errorCallback = callback;
  }

  registerContinuityStore(store: ContinuityStore): void {
    this._continuityStore = store;
  }

  registerLeaseProvider(provider: LeaseProvider): void {
    this._leaseProvider = provider;
  }

  registerRosterProvider(provider: RosterProvider): void {
    this._rosterProvider = provider;
  }

  registerAgentCustomizer(customizer: AgentCustomizer): void {
    this._agentCustomizer = customizer;
  }

  registerTopologyProvider(provider: TopologyProvider): void {
    this._topologyProvider = provider;
  }

  /** Remove all tool handlers for a scope. Call when a session ends. */
  releaseScope(scopeId: string): void {
    const tools = this._scopeTools.get(scopeId);
    if (tools) {
      for (const toolName of tools) {
        this._toolHandlers.delete(`${scopeId}:${toolName}`);
      }
      this._scopeTools.delete(scopeId);
    }
  }

  async handleCallback(
    method: string,
    params: Record<string, unknown>,
  ): Promise<unknown> {
    if (method === "mobkit/on_error") {
      if (this._errorCallback !== null) {
        const event = parseErrorEvent(params);
        try {
          await this._errorCallback(event);
        } catch {
          // Fire-and-forget — swallow error callback failures
        }
      }
      return null;
    }

    if (method === "callback/after_create") {
      if (this._builder !== null && typeof this._builder.afterCreate === "function") {
        const sessionId = String(params.session_id ?? "");
        const context: SessionCreatedContext = {
          model: String(params.model ?? ""),
          labels: typeof params.labels === "object" && params.labels !== null
            ? (params.labels as Record<string, string>)
            : {},
          systemPrompt: typeof params.system_prompt === "string"
            ? params.system_prompt
            : null,
        };
        try {
          await this._builder.afterCreate(sessionId, context);
        } catch {
          // Best-effort — swallow afterCreate failures.
        }
      }
      return null;
    }

    if (method === "callback/build_agent") {
      if (this._builder === null) {
        throw new Error("no SessionAgentBuilder registered");
      }
      const rawOptions = {
        ...(typeof params.options === "object" && params.options !== null
          ? (params.options as Record<string, unknown>)
          : {}),
      };
      const scopeId = String(rawOptions.scope_id ?? "");
      if (!scopeId) {
        throw new Error("callback/build_agent requires scope_id in options");
      }
      delete rawOptions.scope_id;

      const opts = new SessionBuildOptions();
      if (rawOptions.app_context !== undefined) {
        opts.appContext = rawOptions.app_context;
      }
      if (Array.isArray(rawOptions.additional_instructions)) {
        opts.additionalInstructions = rawOptions.additional_instructions.filter(
          (v): v is string => typeof v === "string",
        );
      }
      if (typeof rawOptions.session_id === "string") {
        opts.sessionId = rawOptions.session_id;
      }
      if (
        typeof rawOptions.labels === "object" &&
        rawOptions.labels !== null
      ) {
        opts.labels = rawOptions.labels as Record<string, string>;
      }
      if (typeof rawOptions.profile_name === "string") {
        opts.profileName = rawOptions.profile_name;
      }

      await this._builder.buildAgent(opts);

      // Capture tool handlers scoped to this build
      const toolNames: string[] = [];
      for (const [name, handler] of opts.toolHandlers) {
        this._toolHandlers.set(`${scopeId}:${name}`, handler);
        toolNames.push(name);
      }
      this._scopeTools.set(scopeId, toolNames);

      return opts.toDict();
    }

    if (method === "callback/call_tool") {
      const scopeId = String(params.scope_id ?? "");
      if (!scopeId) {
        throw new Error("callback/call_tool requires scope_id");
      }
      const toolName = String(params.tool ?? "");
      const args = (
        typeof params.arguments === "object" && params.arguments !== null
          ? params.arguments
          : {}
      ) as Record<string, unknown>;

      const handler = this._toolHandlers.get(`${scopeId}:${toolName}`);
      if (!handler) {
        throw new Error(
          `no handler registered for tool: ${toolName} (scope: ${scopeId})`,
        );
      }

      const result = await handler(args);
      return { content: result };
    }

    // -- Continuity callbacks -----------------------------------------------

    if (method === "callback/continuity_store/resolve_many") {
      if (this._continuityStore === null) {
        throw new Error("no ContinuityStore registered");
      }
      const identities = Array.isArray(params.identities)
        ? params.identities.map(String)
        : [];
      return this._continuityStore.resolveMany(identities);
    }

    if (method === "callback/continuity_store/load_session_snapshot") {
      if (this._continuityStore === null) {
        throw new Error("no ContinuityStore registered");
      }
      const sessionId = String(params.session_id ?? "");
      const snap = await this._continuityStore.loadSessionSnapshot(sessionId);
      if (snap === null) return null;
      return sessionSnapshotToDict(snap);
    }

    if (method === "callback/continuity_store/delete_session_snapshot_if_current_revision") {
      if (this._continuityStore === null) {
        throw new Error("no ContinuityStore registered");
      }
      const sessionId = String(params.session_id ?? "");
      const expectedCurrentRevision = String(params.expected_current_revision ?? "");
      const handler = this._continuityStore.deleteSessionSnapshotIfCurrentRevision;
      if (!handler) return false;
      return handler.call(this._continuityStore, sessionId, expectedCurrentRevision);
    }

    if (method === "callback/continuity_store/save_session_snapshot") {
      if (this._continuityStore === null) {
        throw new Error("no ContinuityStore registered");
      }
      const identity = String(params.identity ?? "");
      const sessionId = String(params.session_id ?? "");
      const generation = Number(params.generation ?? 0);
      const version = Number(params.version ?? params.checkpoint_version ?? 0);
      const fencingToken = Number(params.fencing_token ?? 0);
      const snapshot = typeof params.snapshot === "string"
        ? parseSessionSnapshot({ data: params.snapshot })
        : parseSessionSnapshot(params.snapshot);
      await this._continuityStore.saveSessionSnapshot(
        identity, sessionId, generation, version, fencingToken, snapshot,
      );
      return null;
    }

    if (method === "callback/continuity_store/upsert_continuity_record") {
      if (this._continuityStore === null) {
        throw new Error("no ContinuityStore registered");
      }
      const record = parseContinuityRecord(params.record);
      const fencingToken = Number(params.fencing_token ?? 0);
      await this._continuityStore.upsertContinuityRecord(record, fencingToken);
      return null;
    }

    if (method === "callback/continuity_store/delete_continuity_record") {
      if (this._continuityStore === null) {
        throw new Error("no ContinuityStore registered");
      }
      const identity = String(params.identity ?? "");
      const fencingToken = Number(params.fencing_token ?? 0);
      await this._continuityStore.deleteContinuityRecord(identity, fencingToken);
      return null;
    }

    // -- Lease callbacks ----------------------------------------------------

    if (method === "callback/lease_provider/acquire_leases") {
      if (this._leaseProvider === null) {
        throw new Error("no LeaseProvider registered");
      }
      const identities = Array.isArray(params.identities)
        ? params.identities.map(String)
        : [];
      const runtimeInstance = String(params.runtime_instance ?? "");
      const result = await this._leaseProvider.acquireLeases(identities, runtimeInstance);
      return Object.fromEntries(
        Object.entries(result).map(([identity, value]) => [
          identity,
          leaseAcquireResultToDict({ identity, ...value }),
        ]),
      );
    }

    if (method === "callback/lease_provider/renew_leases") {
      if (this._leaseProvider === null) {
        throw new Error("no LeaseProvider registered");
      }
      const rawGrants = Array.isArray(params.grants) ? params.grants : [];
      const grants: LeaseGrant[] = rawGrants.map(parseLeaseGrant);
      const result = await this._leaseProvider.renewLeases(grants);
      return Object.fromEntries(
        Object.entries(result).map(([identity, value]) => [
          identity,
          leaseRenewResultToDict({ identity, ...value }),
        ]),
      );
    }

    if (method === "callback/lease_provider/release_leases") {
      if (this._leaseProvider === null) {
        throw new Error("no LeaseProvider registered");
      }
      const rawGrants = Array.isArray(params.grants) ? params.grants : [];
      const grants: LeaseGrant[] = rawGrants.map(parseLeaseGrant);
      await this._leaseProvider.releaseLeases(grants);
      return null;
    }

    // -- Roster callback ----------------------------------------------------

    if (method === "callback/roster_provider/roster") {
      if (this._rosterProvider === null) {
        throw new Error("no RosterProvider registered");
      }
      const context = params.context ?? {};
      const specs = await this._rosterProvider.roster(context);
      return specs.map(durableAgentSpecToDict);
    }

    // -- Topology callback --------------------------------------------------

    if (method === "callback/topology_provider/compute_edges") {
      if (this._topologyProvider === null) {
        throw new Error("no TopologyProvider registered");
      }
      const targetIdentities = Array.isArray(params.target_identities)
        ? params.target_identities.map(String)
        : [];
      const context = params.context ?? {};
      const edges = await this._topologyProvider.computeEdges(
        targetIdentities,
        context,
      );
      return edges.map(managedPeerEdgeToDict);
    }

    // -- Customizer callbacks -----------------------------------------------

    if (method === "callback/agent_customizer/customize_build") {
      if (this._agentCustomizer === null) {
        throw new Error("no AgentCustomizer registered");
      }
      const context = parseAgentBuildContext(params.context);
      const spec = parseDurableAgentSpec(params.spec);
      const draft = parseAgentBuildDraft(params.draft);
      await this._agentCustomizer.customizeBuild(context, spec, draft);
      return agentBuildDraftToDict(draft);
    }

    if (method === "callback/agent_customizer/after_create") {
      if (
        this._agentCustomizer !== null &&
        typeof this._agentCustomizer.afterCreate === "function"
      ) {
        const identity = String(params.identity ?? "");
        const sessionId = String(params.session_id ?? "");
        const rawLabels = (params.labels != null && typeof params.labels === "object")
          ? Object.fromEntries(
              Object.entries(params.labels as Record<string, unknown>).map(([k, v]) => [k, String(v)])
            )
          : {};
        const context: SessionCreatedContext = {
          model: String(params.model ?? ""),
          labels: rawLabels,
          systemPrompt: typeof params.system_prompt === "string" ? params.system_prompt : null,
        };
        try {
          await this._agentCustomizer.afterCreate(
            identity,
            sessionId,
            context,
          );
        } catch {
          // Best-effort — swallow afterCreate failures.
        }
      }
      return null;
    }

    throw new Error(`unknown callback method: ${method}`);
  }
}
