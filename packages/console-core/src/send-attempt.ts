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
  /** Typed reason for a settled failure; `error` is its rendered message. */
  failureKind?: ConsoleSendFailureKind;
  accepted?: { interactionId: string; inputFrameId?: string };
}

/** Why a send attempt did not produce an acceptance receipt. Classified from
 * typed transport facts (HTTP status, JSON-RPC code or `data.kind`, fetch
 * layer failure kind), never from message text. */
export type ConsoleSendFailureKind =
  | "unauthenticated"
  | "access_denied"
  | "read_only"
  | "rejected"
  | "refused"
  | "rate_limited"
  | "interrupted"
  | "capability_unavailable"
  | "unreachable"
  | "timeout"
  | "invalid_response"
  | "gateway_error"
  | "unknown";
export interface ConsoleSendFailure {
  state: "definitely-rejected" | "outcome-unknown";
  kind: ConsoleSendFailureKind;
  /** Operator-facing sentence naming the reason and what to do next. */
  message: string;
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
    // An unrecognized kind (a newer tab's vocabulary) renders with the
    // state's generic label rather than blocking the whole saved queue.
    (value.failureKind !== undefined && (!nonemptyString(value.failureKind) ||
      (value.state !== "definitely-rejected" && value.state !== "outcome-unknown"))) ||
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
    lease: { owner: options.owner, expiresAt: options.now + 15_000 }, error: undefined, failureKind: undefined };
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
  | { state: "definitely-rejected" | "outcome-unknown"; error: string; kind?: ConsoleSendFailureKind },
): ConsoleSendAttempt {
  validateConsoleSendAttempt(attempt);
  if (attempt.state === "accepted") return attempt;
  if (!attempt.envelopeJson) throw new Error("A draft has not been dispatched.");
  if (result.state === "accepted") {
    if (!nonemptyString(result.interactionId) || (result.inputFrameId !== undefined && !nonemptyString(result.inputFrameId))) throw new Error("Server response did not prove acceptance.");
    return { ...attempt, state: "accepted", lease: undefined, error: undefined, failureKind: undefined,
      accepted: { interactionId: result.interactionId, inputFrameId: result.inputFrameId } };
  }
  return { ...attempt, state: result.state, lease: undefined, error: result.error, failureKind: result.kind };
}

type TypedRpcError = { code?: unknown; message?: unknown; data?: { kind?: unknown } | null };
interface TypedSendError {
  rpcError?: TypedRpcError;
  responseRpcError?: TypedRpcError;
  httpStatus?: unknown;
  transportFailure?: unknown;
  timeoutMs?: unknown;
  message?: unknown;
  name?: unknown;
}

/** HTTP statuses the gateway (or a fronting proxy) answers before any
 * reservation: the request was refused as a request, so nothing was sent.
 * 408/409/429/5xx and everything unlisted stay unknown (the console REST
 * send answers 429 after its reservation, when the member's admission
 * backlog is full). */
const PRE_INGRESS_REFUSAL_STATUSES = new Set([400, 404, 405, 413, 414, 415, 422, 431]);

/**
 * Classify a failed send into a typed state, kind and operator message.
 *
 * Only a refusal that proves the request never reached dispatch is
 * `definitely-rejected`: a 401 or typed `unauthenticated` refusal, a 403 or
 * typed `access_denied`, a typed read-only refusal, invalid params (-32602),
 * or a listed pre-ingress HTTP status. Network loss, timeouts, 5xx,
 * non-JSON answers and conflicts are `outcome-unknown`: the server may have
 * reserved the message, so the attempt keeps its envelope for reconciliation.
 */
export function classifyConsoleSendFailure(error: unknown): ConsoleSendFailure {
  const typed = (error && typeof error === "object" ? error : {}) as TypedSendError;
  const rpc = typed.rpcError ?? typed.responseRpcError;
  const rpcKind = typeof rpc?.data?.kind === "string" ? rpc.data.kind : undefined;
  const rpcMessage = typeof rpc?.message === "string" && rpc.message.trim() ? rpc.message.trim() : undefined;
  const status = typeof typed.httpStatus === "number" ? typed.httpStatus : undefined;
  const detail = error instanceof Error && error.message.trim() ? error.message.trim()
    : typeof typed.message === "string" && typed.message.trim() ? typed.message.trim() : undefined;
  if (rpcKind === "unauthenticated" || status === 401) {
    return { state: "definitely-rejected", kind: "unauthenticated",
      message: "Not authorized from this network (401). The gateway refused the request before accepting it, so nothing was sent. Sign in or connect from a trusted network, then retry." };
  }
  if (rpcKind === "access_denied" || rpc?.code === -32030 || status === 403) {
    return { state: "definitely-rejected", kind: "access_denied",
      message: `Not allowed to send to this agent (403)${rpcMessage ? `: ${rpcMessage}` : ""}. Nothing was sent.` };
  }
  if (rpcKind === "read_only") {
    return { state: "definitely-rejected", kind: "read_only",
      message: "The console is read-only. Nothing was sent." };
  }
  if (rpc?.code === -32602) {
    return { state: "definitely-rejected", kind: "rejected",
      message: `Send rejected: ${rpcMessage ?? detail ?? "invalid request"}. Nothing was sent.` };
  }
  // The console's own lifetime ended (a reload, remount or account switch)
  // while the request was in flight: the answer was discarded, so the send
  // may well have been accepted. Typed by the abort's DOMException name.
  if (typed.name === "AbortError" && typed.transportFailure === undefined) {
    return { state: "outcome-unknown", kind: "interrupted",
      message: "The console was reloaded or switched while this send was in flight, so its answer was not received. It may have been accepted: check acceptance before retrying." };
  }
  if (typed.transportFailure === "timeout") {
    const seconds = typeof typed.timeoutMs === "number" ? ` within ${Math.round(typed.timeoutMs / 1000)} s` : "";
    return { state: "outcome-unknown", kind: "timeout",
      message: `No response from the gateway${seconds}. It may still have accepted the message: check acceptance before retrying.` };
  }
  if (typed.transportFailure === "unreachable") {
    return { state: "outcome-unknown", kind: "unreachable",
      message: `Gateway unreachable${detail ? ` (${detail})` : ""}. The console could not confirm whether the message arrived: check acceptance once the connection is back.` };
  }
  if (typed.transportFailure === "invalid_response") {
    return { state: "outcome-unknown", kind: "invalid_response",
      message: `${detail ?? "The gateway returned an unreadable response"}. Something between the console and the gateway answered instead of it: check acceptance before retrying.` };
  }
  if (status !== undefined && PRE_INGRESS_REFUSAL_STATUSES.has(status)) {
    return { state: "definitely-rejected", kind: "rejected",
      message: `Send rejected by the gateway (HTTP ${status})${rpcMessage ? `: ${rpcMessage}` : ""}. Nothing was sent.` };
  }
  // The gateway answered the send with a typed JSON-RPC error that does not
  // prove pre-reservation refusal (e.g. a steer that was reserved and then
  // refused by the member, or an idempotency conflict): name the reason, but
  // keep the attempt reconcilable.
  if (typed.rpcError && (rpcMessage || detail)) {
    return { state: "outcome-unknown", kind: "refused",
      message: `Send failed: ${rpcMessage ?? detail}. The gateway may have recorded this attempt: check acceptance before retrying.` };
  }
  if (status === 429) {
    return { state: "outcome-unknown", kind: "rate_limited",
      message: "The agent is not taking more input right now (HTTP 429). It may already have recorded this attempt: check acceptance before retrying." };
  }
  if (status !== undefined && status >= 500) {
    return { state: "outcome-unknown", kind: "gateway_error",
      message: `Gateway error (HTTP ${status}). The console could not confirm acceptance: check acceptance before retrying.` };
  }
  return { state: "outcome-unknown", kind: "unknown",
    message: `The console could not confirm acceptance${detail ? `: ${detail}` : ""}. Check acceptance before retrying.` };
}

/** Only structured pre-ingress rejection is proof. Network errors and conflicts are unknown. */
export function consoleSendFailureState(error: unknown): "definitely-rejected" | "outcome-unknown" {
  return classifyConsoleSendFailure(error).state;
}

/** Short row label for a settled attempt, from its typed failure kind. */
export function consoleSendFailureLabel(attempt: Pick<ConsoleSendAttempt, "state" | "failureKind">): string {
  switch (attempt.failureKind) {
    case "unauthenticated": return "Not authorized";
    case "access_denied": return "Not allowed";
    case "read_only": return "Console read-only";
    case "capability_unavailable": return "Send unavailable";
    case "unreachable": return "Gateway unreachable";
    case "timeout": return "No response";
    case "invalid_response": return "Unreadable response";
    case "gateway_error": return "Gateway error";
    case "rejected": return "Rejected";
    case "refused": return "Send failed";
    case "rate_limited": return "Gateway busy";
    case "interrupted": return "Interrupted";
    default: return attempt.state === "definitely-rejected" ? "Not accepted" : "Acceptance unknown";
  }
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
}, resolution?: { requestedIdentity: string; canonicalIdentity: string }): ConsoleSendAttempt | null {
  const destinationResolved = frame.identity === attempt.destination ||
    (resolution?.requestedIdentity === attempt.destination
      && nonemptyString(resolution.canonicalIdentity) && frame.identity === resolution.canonicalIdentity);
  if (!attempt.envelopeJson || frame.event !== "user_input" || !destinationResolved || !frame.interactionId || !frame.id) return null;
  const envelope = JSON.parse(attempt.envelopeJson) as ConsoleFrozenSendEnvelope;
  const payload = frame.data as Partial<ConsoleFrozenSendEnvelope> | null;
  if (!payload || payload.origin !== envelope.origin || payload.origin_kind !== envelope.origin_kind ||
    payload.idempotency_key !== envelope.idempotency_key || payload.handling_mode !== envelope.handling_mode ||
    !sameFrozenContent(payload.content, envelope.content)) return null;
  return finishConsoleSendAttempt(attempt, { state: "accepted", interactionId: frame.interactionId, inputFrameId: frame.id });
}

/** Typed outcome of an explicit "Check acceptance" that did not find a
 * receipt. A failed check never changes the attempt's own state: it proves
 * nothing about the send, and the saved message is never resent by it. */
export interface ConsoleAcceptanceCheckResult {
  kind: ConsoleSendFailureKind | "no_receipt";
  message: string;
}

export const CONSOLE_ACCEPTANCE_NO_RECEIPT: ConsoleAcceptanceCheckResult = {
  kind: "no_receipt",
  message: "No acceptance receipt: this agent's timeline has no record of this message. It remains saved and will not be resent automatically.",
};

export function describeConsoleAcceptanceCheckFailure(error: unknown): ConsoleAcceptanceCheckResult {
  const failure = classifyConsoleSendFailure(error);
  const unchanged = "The saved message is unchanged and was not resent.";
  switch (failure.kind) {
    case "unauthenticated":
      return { kind: failure.kind, message: `Could not check acceptance: not authorized from this network (401). ${unchanged}` };
    case "access_denied":
      return { kind: failure.kind, message: `Could not check acceptance: not allowed to read this agent's timeline (403). ${unchanged}` };
    case "unreachable":
      return { kind: failure.kind, message: `Could not check acceptance: gateway unreachable. ${unchanged}` };
    case "timeout":
      return { kind: failure.kind, message: `Could not check acceptance: no response from the gateway. ${unchanged}` };
    default: {
      const status = (error as { httpStatus?: unknown } | null)?.httpStatus;
      const detail = error instanceof Error && error.message.trim() ? error.message.trim() : "the check failed";
      return { kind: failure.kind, message: `Could not check acceptance${typeof status === "number" ? ` (HTTP ${status})` : ""}: ${detail}. ${unchanged}` };
    }
  }
}
