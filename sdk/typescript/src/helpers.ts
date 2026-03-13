/**
 * Module authoring helpers and console route builders.
 *
 * These are public utilities for defining MobKit modules, tools, and
 * admin console routes. Matches the Python SDK's `meerkat_mobkit.helpers`.
 */

// -- Types ----------------------------------------------------------------

export type RestartPolicy = "never" | "always" | "on_failure";

export interface ModuleSpec {
  readonly id: string;
  readonly command: string;
  readonly args: readonly string[];
  readonly restart_policy: RestartPolicy;
}

export type ModuleSpecDecorator = (spec: ModuleSpec) => ModuleSpec;

export interface ModuleToolContext {
  readonly moduleId: string;
  readonly requestId: string;
}

export type ModuleToolHandler<TInput = unknown, TOutput = unknown> = (
  input: TInput,
  context: ModuleToolContext,
) => Promise<TOutput> | TOutput;

export type ModuleToolDecorator<TInput = unknown, TOutput = unknown> = (
  next: ModuleToolHandler<TInput, TOutput>,
) => ModuleToolHandler<TInput, TOutput>;

export interface ModuleToolDefinition<TInput = unknown, TOutput = unknown> {
  readonly name: string;
  readonly description?: string;
  readonly handler: ModuleToolHandler<TInput, TOutput>;
}

export interface ModuleDefinition {
  readonly spec: ModuleSpec;
  readonly description?: string;
  readonly tools: readonly ModuleToolDefinition[];
}

export interface ConsoleRoutes {
  readonly modules: string;
  readonly experience: string;
}

// -- Module spec helpers --------------------------------------------------

export function defineModuleSpec(input: {
  id: string;
  command: string;
  args?: string[];
  restartPolicy?: RestartPolicy;
}): ModuleSpec {
  return {
    id: input.id,
    command: input.command,
    args: input.args ?? [],
    restart_policy: input.restartPolicy ?? "never",
  };
}

export function decorateModuleSpec(
  spec: ModuleSpec,
  ...decorators: ModuleSpecDecorator[]
): ModuleSpec {
  const base: ModuleSpec = { ...spec, args: [...spec.args] };
  return decorators.reduce((current, decorate) => decorate(current), base);
}

// -- Module tool helpers --------------------------------------------------

export function decorateModuleTool<TInput = unknown, TOutput = unknown>(
  handler: ModuleToolHandler<TInput, TOutput>,
  ...decorators: ModuleToolDecorator<TInput, TOutput>[]
): ModuleToolHandler<TInput, TOutput> {
  return decorators.reduceRight((next, decorate) => decorate(next), handler);
}

export function defineModuleTool<TInput = unknown, TOutput = unknown>(input: {
  name: string;
  handler: ModuleToolHandler<TInput, TOutput>;
  description?: string;
  decorators?: ModuleToolDecorator<TInput, TOutput>[];
}): ModuleToolDefinition<TInput, TOutput> {
  return {
    name: input.name,
    description: input.description,
    handler: decorateModuleTool(input.handler, ...(input.decorators ?? [])),
  };
}

// -- Module definition ----------------------------------------------------

export function defineModule(input: {
  spec: ModuleSpec;
  description?: string;
  tools?: ModuleToolDefinition[];
}): ModuleDefinition {
  return {
    spec: { ...input.spec, args: [...input.spec.args] },
    description: input.description,
    tools: [...(input.tools ?? [])],
  };
}

// -- Console route helpers ------------------------------------------------

function appendAuthToken(path: string, authToken?: string): string {
  if (!authToken) {
    return path;
  }
  const joiner = path.includes("?") ? "&" : "?";
  return `${path}${joiner}auth_token=${encodeURIComponent(authToken)}`;
}

export function buildConsoleRoute(
  path: "/console/modules" | "/console/experience",
  authToken?: string,
): string {
  return appendAuthToken(path, authToken);
}

export function buildConsoleModulesRoute(authToken?: string): string {
  return buildConsoleRoute("/console/modules", authToken);
}

export function buildConsoleExperienceRoute(authToken?: string): string {
  return buildConsoleRoute("/console/experience", authToken);
}

export function buildConsoleRoutes(authToken?: string): ConsoleRoutes {
  return {
    modules: buildConsoleModulesRoute(authToken),
    experience: buildConsoleExperienceRoute(authToken),
  };
}
