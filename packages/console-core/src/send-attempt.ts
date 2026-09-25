import { serializeConsoleContextMessage, validateConsoleContexts, type ConsoleContextRecord } from "./context-record";

export type ConsoleSendAttemptState = "draft" | "attempting" | "accepted" | "definitely-rejected" | "outcome-unknown";
export interface ConsoleFrozenSendEnvelope {
  identity: string;
  content: string | Array<{ type: "text"; text: string }>;
  origin: string;
  origin_kind: "operator";
  idempotency_key: string;
  handling_mode: "queue" | "steer";
}
export interface ConsoleSendAttempt {
  version: 1;
  id: string;
  scope: string;
  destination: string;
  origin: string;
  idempotencyKey: string;
  text: string;
  contexts: ConsoleContextRecord[];
  addedAt: number;
  state: ConsoleSendAttemptState;
  /** Persisted serialized envelope is immutable after the first attempt. */
  envelopeJson?: string;
  lease?: { owner: string; expiresAt: number };
  error?: string;
  accepted?: { interactionId: string; inputFrameId?: string };
}
const nonemptyString = (value: unknown): value is string => typeof value === "string" && value.trim().length > 0;

export function createConsoleSendAttempt(input: {
  id: string; scope: string; destination: string; origin: string; idempotencyKey: string;
  text: string; contexts?: ConsoleContextRecord[]; now: number;
}): ConsoleSendAttempt {
  if (!input.text.trim()) throw new Error("Write a message before sending.");
  const attempt: ConsoleSendAttempt = {
    version: 1, id: input.id, scope: input.scope, destination: input.destination,
    origin: input.origin, idempotencyKey: input.idempotencyKey, text: input.text,
    contexts: structuredClone(input.contexts ?? []), addedAt: input.now, state: "draft",
  };
  validateConsoleSendAttempt(attempt);
  return attempt;
}

export function validateConsoleSendAttempt(value: ConsoleSendAttempt): void {
  if (!value || value.version !== 1 || !nonemptyString(value.id) || !nonemptyString(value.scope) || !nonemptyString(value.destination) || !nonemptyString(value.origin) ||
    !nonemptyString(value.idempotencyKey) || typeof value.text !== "string" || !value.text.trim() ||
    !Number.isFinite(value.addedAt) || !Array.isArray(value.contexts) ||
    !["draft", "attempting", "accepted", "definitely-rejected", "outcome-unknown"].includes(value.state)) {
    throw new Error("The saved send attempt is invalid or uses an unsupported version.");
  }
  if ((value.error !== undefined && typeof value.error !== "string") ||
    (value.lease !== undefined && (!value.lease || !nonemptyString(value.lease.owner) || !Number.isFinite(value.lease.expiresAt))) ||
    (value.state === "attempting" && !value.lease) || (value.state !== "attempting" && value.lease !== undefined) ||
    (value.state === "accepted" && (!value.accepted || !nonemptyString(value.accepted.interactionId) ||
      (value.accepted.inputFrameId !== undefined && !nonemptyString(value.accepted.inputFrameId)))) ||
    (value.state !== "accepted" && value.accepted !== undefined)) {
    throw new Error("The saved send lease or acceptance receipt is invalid.");
  }
  validateConsoleContexts(value.contexts);
  if (value.envelopeJson !== undefined && !nonemptyString(value.envelopeJson)) throw new Error("The saved send envelope is invalid.");
  if (value.state !== "draft" && !nonemptyString(value.envelopeJson)) throw new Error("A send attempt is missing its frozen envelope.");
  if (value.state === "draft" && value.envelopeJson) throw new Error("An attempted envelope cannot be changed back into a draft.");
  if (value.envelopeJson) {
    const envelope = JSON.parse(value.envelopeJson) as ConsoleFrozenSendEnvelope;
    const content = value.contexts.length ? serializeConsoleContextMessage(value.text, value.contexts) : value.text;
    if (envelope.identity !== value.destination || envelope.origin !== value.origin ||
      envelope.idempotency_key !== value.idempotencyKey || envelope.origin_kind !== "operator" ||
      !["queue", "steer"].includes(envelope.handling_mode) || JSON.stringify(envelope.content) !== JSON.stringify(content)) {
      throw new Error("The saved send envelope does not match its original intent.");
    }
  }
}

export function beginConsoleSendAttempt(
  attempt: ConsoleSendAttempt,
  options: { owner: string; now: number; handlingMode: "queue" | "steer"; retryRejected?: boolean },
): ConsoleSendAttempt {
  validateConsoleSendAttempt(attempt);
  if (!nonemptyString(options.owner) || !Number.isFinite(options.now)) throw new Error("The browser send lease is invalid.");
  if (attempt.state !== "draft" && !(attempt.state === "definitely-rejected" && options.retryRejected)) {
    throw new Error("This send may already have been accepted. Reconcile it before taking another action.");
  }
  const envelope: ConsoleFrozenSendEnvelope = {
    identity: attempt.destination,
    content: attempt.contexts.length ? serializeConsoleContextMessage(attempt.text, attempt.contexts) : attempt.text,
    origin: attempt.origin, origin_kind: "operator", idempotency_key: attempt.idempotencyKey,
    handling_mode: options.handlingMode,
  };
  if (attempt.envelopeJson && JSON.parse(attempt.envelopeJson).handling_mode !== options.handlingMode) {
    throw new Error("An attempted message cannot change handling mode.");
  }
  return { ...attempt, state: "attempting", envelopeJson: attempt.envelopeJson ?? JSON.stringify(envelope),
    lease: { owner: options.owner, expiresAt: options.now + 15_000 }, error: undefined };
}

/** Expiring a browser lease never proves that the server did not accept a send. */
export function recoverConsoleSendAttempt(attempt: ConsoleSendAttempt, now: number): ConsoleSendAttempt {
  validateConsoleSendAttempt(attempt);
  if (attempt.state === "attempting" && (!attempt.lease || attempt.lease.expiresAt <= now)) {
    return { ...attempt, state: "outcome-unknown", lease: undefined,
      error: "Acceptance is unknown. Check the conversation before explicitly discarding this attempt." };
  }
  return attempt;
}

export function finishConsoleSendAttempt(attempt: ConsoleSendAttempt, result:
  | { state: "accepted"; interactionId: string; inputFrameId?: string }
  | { state: "definitely-rejected" | "outcome-unknown"; error: string },
): ConsoleSendAttempt {
  validateConsoleSendAttempt(attempt);
  if (attempt.state === "accepted") return attempt;
  if (!attempt.envelopeJson) throw new Error("A draft has not been dispatched.");
  if (result.state === "accepted") {
    if (!nonemptyString(result.interactionId) || (result.inputFrameId !== undefined && !nonemptyString(result.inputFrameId))) throw new Error("Server response did not prove acceptance.");
    return { ...attempt, state: "accepted", lease: undefined, error: undefined,
      accepted: { interactionId: result.interactionId, inputFrameId: result.inputFrameId } };
  }
  return { ...attempt, state: result.state, lease: undefined, error: result.error };
}

/** Only structured pre-ingress rejection is proof. Network errors and conflicts are unknown. */
export function consoleSendFailureState(error: unknown): "definitely-rejected" | "outcome-unknown" {
  const rpc = (error as { rpcError?: { code?: unknown; data?: { kind?: unknown } } })?.rpcError;
  return rpc?.code === -32602 || rpc?.data?.kind === "access_denied"
    ? "definitely-rejected" : "outcome-unknown";
}

function sameFrozenContent(actual: unknown, expected: ConsoleFrozenSendEnvelope["content"]): boolean {
  if (typeof expected === "string") return actual === expected;
  if (!Array.isArray(actual) || actual.length !== expected.length) return false;
  return actual.every((block, index) => block !== null && typeof block === "object" &&
    Object.keys(block).length === 2 && block.type === expected[index].type && block.text === expected[index].text);
}

/** Reconcile only an exact owner user_input receipt from an authorized timeline. */
export function reconcileConsoleSendReceipt(attempt: ConsoleSendAttempt, frame: {
  id: string; event: string; identity?: string; interactionId?: string; data: unknown;
}): ConsoleSendAttempt | null {
  if (!attempt.envelopeJson || frame.event !== "user_input" || frame.identity !== attempt.destination || !frame.interactionId || !frame.id) return null;
  const envelope = JSON.parse(attempt.envelopeJson) as ConsoleFrozenSendEnvelope;
  const payload = frame.data as Partial<ConsoleFrozenSendEnvelope> | null;
  if (!payload || payload.origin !== envelope.origin || payload.origin_kind !== envelope.origin_kind ||
    payload.idempotency_key !== envelope.idempotency_key || payload.handling_mode !== envelope.handling_mode ||
    !sameFrozenContent(payload.content, envelope.content)) return null;
  return finishConsoleSendAttempt(attempt, { state: "accepted", interactionId: frame.interactionId, inputFrameId: frame.id });
}
