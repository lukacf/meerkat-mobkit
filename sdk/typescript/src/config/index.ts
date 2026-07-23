/**
 * Configuration modules for MobKit runtime.
 *
 * @example
 * ```ts
 * import { auth, memory, sessionStore } from "@rkat/mobkit-sdk";
 *
 * const authConfig = auth.google("my-client-id");
 * const memConfig = memory.localJson();
 * const storeConfig = sessionStore.json("./sessions.json");
 * ```
 */

export * as auth from "./auth.js";
export * as eventLog from "./event-log.js";
export * as memory from "./memory.js";
export * as runtimeStore from "./runtime-store.js";
export * as sessionStore from "./session-store.js";
