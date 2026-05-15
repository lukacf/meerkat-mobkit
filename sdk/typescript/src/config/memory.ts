/**
 * Memory backend configuration for MobKit runtime.
 */

export interface ElephantMemoryConfig {
  readonly endpoint: string;
  readonly spaceId: string | null;
  readonly collection: string | null;
  readonly stores: readonly string[];
  toDict(): Record<string, unknown>;
}

export function elephant(
  endpoint: string,
  options?: {
    spaceId?: string;
    collection?: string;
    stores?: string[];
  },
): ElephantMemoryConfig {
  const config = {
    endpoint,
    spaceId: options?.spaceId ?? null,
    collection: options?.collection ?? null,
    stores: options?.stores ?? [],
    toDict() {
      return elephantMemoryConfigToDict(config);
    },
  };
  return config;
}

export function elephantMemoryConfigToDict(
  config: ElephantMemoryConfig,
): Record<string, unknown> {
  const result: Record<string, unknown> = {
    backend: "elephant",
    endpoint: config.endpoint,
  };
  return result;
}
