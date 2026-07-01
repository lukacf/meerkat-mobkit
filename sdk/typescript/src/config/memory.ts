/**
 * Memory backend configuration for MobKit runtime.
 *
 * The operational memory ledger (`mobkit/memory/*`) is persisted by the
 * gateway as local JSON under `persistent_state`, with an optional HTTP
 * health-check gate. `localJson()` is the honest configuration for that
 * backend; `elephant()` is a deprecated alias kept for wire compatibility
 * with older gateways (it never wrote data to Elephant).
 */

export interface LocalJsonMemoryConfig {
  readonly healthCheckEndpoint: string | null;
  toDict(): Record<string, unknown>;
}

export function localJson(options?: {
  healthCheckEndpoint?: string;
}): LocalJsonMemoryConfig {
  const config = {
    healthCheckEndpoint: options?.healthCheckEndpoint ?? null,
    toDict() {
      const result: Record<string, unknown> = { backend: "local_json" };
      if (config.healthCheckEndpoint !== null) {
        result.health_check_endpoint = config.healthCheckEndpoint;
      }
      return result;
    },
  };
  return config;
}

/** @deprecated Legacy config shape. See {@link elephant}. */
export interface ElephantMemoryConfig {
  readonly endpoint: string;
  readonly spaceId: string | null;
  readonly collection: string | null;
  readonly stores: readonly string[];
  toDict(): Record<string, unknown>;
}

let warnedElephantDeprecated = false;

/**
 * @deprecated Use {@link localJson} instead. Despite the name, this backend
 * never sent data to Elephant: the gateway only health-checks `endpoint` and
 * persists the ledger as local JSON. `spaceId`, `collection`, and `stores`
 * are not sent to the gateway and have never had any effect. The legacy wire
 * shape is still emitted for compatibility with older gateways.
 */
export function elephant(
  endpoint: string,
  options?: {
    spaceId?: string;
    collection?: string;
    stores?: string[];
  },
): ElephantMemoryConfig {
  if (!warnedElephantDeprecated) {
    warnedElephantDeprecated = true;
    console.warn(
      "memory.elephant() is deprecated: it only health-checks the endpoint and " +
        "persists the operational ledger as local JSON; use " +
        "memory.localJson({ healthCheckEndpoint }) instead",
    );
  }
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

/** @deprecated See {@link elephant}. */
export function elephantMemoryConfigToDict(
  config: ElephantMemoryConfig,
): Record<string, unknown> {
  const result: Record<string, unknown> = {
    backend: "elephant",
    endpoint: config.endpoint,
  };
  return result;
}
