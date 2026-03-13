/**
 * Memory backend configuration for MobKit runtime.
 */

export interface ElephantMemoryConfig {
  readonly endpoint: string;
  readonly spaceId: string | null;
  readonly collection: string | null;
  readonly stores: readonly string[];
}

export function elephant(
  endpoint: string,
  options?: {
    spaceId?: string;
    collection?: string;
    stores?: string[];
  },
): ElephantMemoryConfig {
  return {
    endpoint,
    spaceId: options?.spaceId ?? null,
    collection: options?.collection ?? null,
    stores: options?.stores ?? [],
  };
}

export function elephantMemoryConfigToDict(
  config: ElephantMemoryConfig,
): Record<string, unknown> {
  const result: Record<string, unknown> = {
    backend: "elephant",
    endpoint: config.endpoint,
  };
  if (config.spaceId) result.space_id = config.spaceId;
  if (config.collection) result.collection = config.collection;
  if (config.stores.length > 0) result.stores = [...config.stores];
  return result;
}
