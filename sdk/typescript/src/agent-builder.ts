/**
 * SessionAgentBuilder protocol and CallbackDispatcher.
 *
 * The builder protocol uses imperative mutation: buildAgent receives a
 * mutable SessionBuildOptions and modifies it in place.
 */

import { SessionBuildOptions, type ToolHandler } from "./models.js";
import { parseErrorEvent, type ErrorEvent } from "./types.js";

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

  registerBuilder(builder: SessionAgentBuilder): void {
    this._builder = builder;
  }

  registerErrorCallback(callback: ErrorCallback): void {
    this._errorCallback = callback;
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

    throw new Error(`unknown callback method: ${method}`);
  }
}
