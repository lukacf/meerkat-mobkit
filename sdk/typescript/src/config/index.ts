/**
 * Configuration modules for MobKit runtime.
 *
 * @example
 * ```ts
 * import { auth, memory, sessionStore } from "@rkat/mobkit-sdk";
 *
 * const authConfig = auth.google("my-client-id");
 * const memConfig = memory.elephant("http://elephant:8080");
 * const storeConfig = sessionStore.json("./sessions.json");
 * ```
 */

export * as auth from "./auth.js";
export * as memory from "./memory.js";
export * as sessionStore from "./session-store.js";
