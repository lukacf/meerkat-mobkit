import {
  normalizeConsoleInteractionRejectedError,
  normalizeConsoleInteractionAccepted,
  normalizeReplayUnavailableError,
} from "@console-core";
import type {
  ConsoleDockAddressedTarget,
  ConsoleFrame,
  ConsoleIdentityStreamEvent,
  ConsoleInteractAccepted,
  ConsoleReplayUnavailablePayload,
  ConsoleSessionHistoryPage,
  ConsoleSendMessageResult,
  ConsoleGatewayInteractionRejectedError,
} from "../types";

function unwrapConsoleEnvelope(
  eventName: string,
  data: unknown,
): { id?: string; event?: string; identity?: string; interactionId?: string; timestampMs?: number; data: unknown } {
  if (!data || typeof data !== "object") {
    return { data };
  }
  const record = data as Record<string, unknown>;
  if (
    typeof record.event_id === "string" &&
    typeof record.event_type === "string" &&
    typeof record.identity === "string" &&
    "data" in record
  ) {
    const envelope = record as ConsoleIdentityStreamEvent;
    return {
      id: envelope.event_id,
      event: envelope.event_type || eventName,
      identity: envelope.identity,
      interactionId: envelope.interaction_id,
      timestampMs: envelope.timestamp_ms,
      data: envelope.data,
    };
  }
  return { data };
}

export function parseSseFrames(rawText: string): ConsoleFrame[] {
  const blocks = rawText
    .split(/\n\n+/)
    .map((part) => part.trim())
    .filter(Boolean);
  const frames: ConsoleFrame[] = [];

  for (const block of blocks) {
    const lines = block.split("\n");
    let id = "";
    let event = "message";
    const dataLines: string[] = [];

    for (const line of lines) {
      if (line.startsWith("id:")) {
        id = line.slice(3).trim();
        continue;
      }
      if (line.startsWith("event:")) {
        event = line.slice(6).trim();
        continue;
      }
      if (line.startsWith("data:")) {
        dataLines.push(line.slice(5).trim());
      }
    }

    if (!id && dataLines.length === 0) {
      continue;
    }

    const rawData = dataLines.join("\n");
    let data: unknown = rawData;
    if (rawData) {
      try {
        data = JSON.parse(rawData);
      } catch (_) {
        data = rawData;
      }
    }

    const normalized = unwrapConsoleEnvelope(event, data);
    frames.push({
      id: normalized.id || id,
      event: normalized.event || event,
      identity: normalized.identity,
      interactionId: normalized.interactionId,
      timestampMs: normalized.timestampMs,
      data: normalized.data,
    });
  }

  return frames;
}

export async function fetchJson<T>(baseUrl: string, path: string): Promise<T> {
  const response = await fetch(`${baseUrl}${path}`);
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`Request failed ${response.status} for ${path}: ${text}`);
  }
  return response.json() as Promise<T>;
}

async function rpc<T>(
  baseUrl: string,
  method: string,
  params: Record<string, unknown>
): Promise<T> {
  const response = await fetch(`${baseUrl}/console/rpc`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: `${method}:${Date.now()}`,
      method,
      params,
    }),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`${method} request failed ${response.status}: ${text}`);
  }

  const result = await response.json();
  if (result.error) {
    const typedError = normalizeConsoleInteractionRejectedError(result.error) as ConsoleGatewayInteractionRejectedError | null;
    if (typedError) {
      const error = new Error(`${method} RPC error ${typedError.code}: ${typedError.message}`);
      (error as Error & { rpcError?: ConsoleGatewayInteractionRejectedError }).rpcError = typedError;
      throw error;
    }
    throw new Error(`${method} RPC error: ${result.error.message || JSON.stringify(result.error)}`);
  }

  return result.result as T;
}

export async function sendMessage(
  baseUrl: string,
  memberId: string,
  message: string
): Promise<ConsoleSendMessageResult> {
  return rpc<ConsoleSendMessageResult>(baseUrl, "mobkit/send_message", {
    member_id: memberId,
    message,
  });
}

const TERMINAL_SSE_EVENTS = new Set([
  "interaction_complete",
  "run_completed",
  "interaction_failed",
  "run_failed",
]);

interface TerminalCorrelation {
  sessionId?: string;
  interactionId?: string;
}

interface StreamFramesOptions {
  correlation?: TerminalCorrelation;
  onFrame?: (frame: ConsoleFrame) => void;
  stopOnTerminal?: boolean;
}

function matchesCorrelation(
  candidate: unknown,
  correlation?: TerminalCorrelation,
  allowUnscoped = true,
): boolean {
  if (!correlation?.sessionId && !correlation?.interactionId) {
    return true;
  }
  if (candidate === null || typeof candidate !== "object") {
    return allowUnscoped;
  }
  const record = candidate as Record<string, unknown>;
  const sessionId = record.session_id ?? record.sessionId;
  const interactionId = record.interaction_id ?? record.interactionId;
  const hasScopedField = sessionId !== undefined || interactionId !== undefined;
  if (!hasScopedField) {
    return allowUnscoped;
  }
  if (correlation.sessionId && sessionId === correlation.sessionId) {
    return true;
  }
  if (correlation.interactionId && interactionId === correlation.interactionId) {
    return true;
  }
  return false;
}

/**
 * Scan complete SSE blocks (delimited by double-newline) for a terminal
 * event: line.  The last block is skipped because it may be incomplete.
 * If `sessionId` is supplied, only a terminal event whose JSON data carries
 * a matching `session_id` field stops the stream; terminals from other
 * sessions (concurrent turns, other clients) are ignored so they cannot
 * prematurely satisfy the stop condition.
 */
function hasMatchingTerminalEvent(rawText: string, correlation?: TerminalCorrelation): boolean {
  const blocks = rawText.split(/\n\n+/);
  for (let i = 0; i < blocks.length - 1; i++) {
    const block = blocks[i].trim();
    if (!block) continue;
    let eventName = "";
    const dataLines: string[] = [];
    for (const line of block.split("\n")) {
      if (line.startsWith("event:")) eventName = line.slice(6).trim();
      else if (line.startsWith("data:")) dataLines.push(line.slice(5).trim());
    }
    if (!TERMINAL_SSE_EVENTS.has(eventName)) continue;
    if (!correlation?.sessionId && !correlation?.interactionId) return true;
    try {
      const data = JSON.parse(dataLines.join("\n")) as Record<string, unknown>;
      if (matchesCorrelation(data, correlation, false)) return true;
    } catch {
      // unparseable data — treat as unmatched
    }
  }
  return false;
}

async function drainInteractionResponse(
  response: Response,
  correlation?: TerminalCorrelation,
): Promise<ConsoleFrame[]> {
  return streamFramesFromResponse(response, { correlation });
}

async function streamFramesFromResponse(
  response: Response,
  options: StreamFramesOptions = {},
): Promise<ConsoleFrame[]> {
  const stopOnTerminal = options.stopOnTerminal ?? Boolean(options.correlation);
  if (!response.ok) {
    const text = await response.text();
    let parsed: unknown = null;
    try {
      parsed = JSON.parse(text);
    } catch {
      parsed = null;
    }
    const replayError = normalizeReplayUnavailableError(parsed) as ConsoleReplayUnavailablePayload | null;
    if (replayError) {
      const error = new Error(
        `interaction stream replay unavailable for ${replayError.stream}: ${replayError.requested_last_event_id} -> ${replayError.latest_event_id}`,
      );
      (error as Error & { replayError?: ConsoleReplayUnavailablePayload }).replayError = replayError;
      throw error;
    }
    throw new Error(`interaction stream request failed ${response.status}: ${text}`);
  }

  if (!response.body || typeof response.body.getReader !== "function") {
    const frames = parseSseFrames(await response.text());
    for (const frame of frames) {
      if (matchesCorrelation(frame, options.correlation, true)) {
        options.onFrame?.(frame);
      }
    }
    return !options.correlation
      ? frames
      : frames.filter((frame) => matchesCorrelation(frame, options.correlation, true));
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let frameBuffer = "";
  const frames: ConsoleFrame[] = [];

  try {
    while (true) {
      const { value, done } = await reader.read();
      if (done) {
        break;
      }
      const chunk = decoder.decode(value, { stream: true });
      frameBuffer += chunk;
      let sawTerminal = false;
      frameBuffer = flushSseBlocks(frameBuffer, (frame) => {
        if (matchesCorrelation(frame, options.correlation, true)) {
          frames.push(frame);
          options.onFrame?.(frame);
          if (stopOnTerminal && TERMINAL_SSE_EVENTS.has(frame.event || "")) {
            sawTerminal = true;
          }
        }
      });
      if (sawTerminal) {
        break;
      }
    }
    const finalChunk = decoder.decode();
    frameBuffer += finalChunk;
    frameBuffer = flushSseBlocks(frameBuffer, (frame) => {
      if (matchesCorrelation(frame, options.correlation, true)) {
        frames.push(frame);
        options.onFrame?.(frame);
      }
    });
    flushTrailingSseBlock(frameBuffer, (frame) => {
      if (matchesCorrelation(frame, options.correlation, true)) {
        frames.push(frame);
        options.onFrame?.(frame);
      }
    });
  } finally {
    try {
      await reader.cancel();
    } catch {
      // Reader cancellation is best-effort only.
    }
  }

  return frames;
}

function flushSseBlocks(buffer: string, onFrame: (frame: ConsoleFrame) => void): string {
  let searchIndex = 0;
  while (true) {
    const boundaryIndex = buffer.indexOf("\n\n", searchIndex);
    if (boundaryIndex === -1) {
      break;
    }
    const block = buffer.slice(0, boundaryIndex + 2);
    buffer = buffer.slice(boundaryIndex + 2);
    searchIndex = 0;
    for (const frame of parseSseFrames(block)) {
      onFrame(frame);
    }
  }
  return buffer;
}

function flushTrailingSseBlock(buffer: string, onFrame: (frame: ConsoleFrame) => void) {
  if (!buffer.trim()) {
    return;
  }
  for (const frame of parseSseFrames(`${buffer}\n\n`)) {
    onFrame(frame);
  }
}

export async function observeInteraction(
  baseUrl: string,
  memberId: string
): Promise<ConsoleFrame[]> {
  const response = await fetch(`${baseUrl}/interactions/stream`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ member_id: memberId }),
  });
  return drainInteractionResponse(response);
}

export async function observeIdentityInteraction(
  baseUrl: string,
  identity: string,
): Promise<ConsoleFrame[]> {
  const response = await fetch(`${baseUrl}/console/identity/stream`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ identity }),
  });
  return drainInteractionResponse(response);
}

function persistedEventToFrame(raw: unknown, index: number): ConsoleFrame {
  const record = typeof raw === "object" && raw !== null
    ? raw as Record<string, unknown>
    : {};
  if (
    typeof record.event_id === "string"
    && typeof record.event_type === "string"
    && typeof record.identity === "string"
    && "data" in record
  ) {
    return {
      id: String(record.event_id),
      event: String(record.event_type),
      identity: String(record.identity),
      ...(typeof record.interaction_id === "string" ? { interactionId: String(record.interaction_id) } : {}),
      ...(typeof record.timestamp_ms === "number" ? { timestampMs: record.timestamp_ms } : {}),
      data: record.data,
    };
  }
  const event = typeof record.event === "object" && record.event !== null
    ? record.event as Record<string, unknown>
    : {};

  // UnifiedEvent is serde(tag = "kind", rename_all = "snake_case"), so the
  // wire format is {"kind": "agent", ...} / {"kind": "module", ...}.
  if (event.kind === "agent") {
    const payload =
      typeof event.payload === "object" && event.payload !== null
        ? event.payload
        : null;
    return {
      id: String(record.id ?? `event:${index}`),
      event: String(event.event_type ?? "agent_event"),
      ...(typeof record.timestamp_ms === "number" ? { timestampMs: record.timestamp_ms } : {}),
      data: payload ?? event,
    };
  }

  if (event.kind === "module") {
    return {
      id: String(record.id ?? `event:${index}`),
      event: String(event.event_type ?? "module_event"),
      ...(typeof record.timestamp_ms === "number" ? { timestampMs: record.timestamp_ms } : {}),
      data: (event.payload as unknown) ?? event,
    };
  }

  return {
    id: String(record.id ?? `event:${index}`),
    event: String(record.type ?? "event"),
    ...(typeof record.timestamp_ms === "number" ? { timestampMs: record.timestamp_ms } : {}),
    data: raw,
  };
}

/**
 * Result of a `mobkit/query_events` call.
 *
 * `available: false` means the runtime has no `EventLogStore`
 * configured — the server has nothing to replay. Callers MUST NOT
 * use this as a signal to clear local live-overlay state, because
 * the overlay is the only source of truth in that case. Pre-fix the
 * caller wiped the overlay on every terminal event and the rich
 * transcript flickered into a near-empty replay.
 */
export interface QueryEventsResult {
  readonly frames: ConsoleFrame[];
  readonly available: boolean;
}

export async function queryEvents(
  baseUrl: string,
  target: { memberId?: string; identity?: string },
  limit = 40
): Promise<QueryEventsResult> {
  const identity = target.identity?.trim();
  const memberId = target.memberId?.trim();
  const result = await rpc<unknown>(baseUrl, "mobkit/query_events", {
    limit,
    ...(identity ? { identity } : {}),
    ...(identity ? {} : memberId ? { member_id: memberId } : {}),
  });

  let events = result;
  let available = true;
  if (typeof result === "object" && result !== null) {
    const record = result as Record<string, unknown>;
    if (record.status === "no_event_log_configured") {
      events = Array.isArray(record.events) ? record.events : [];
      available = false;
    } else if (Array.isArray(record.events)) {
      events = record.events;
    }
  }

  if (!Array.isArray(events)) {
    return { frames: [], available };
  }

  const frames = events
    .filter((raw) => {
      if (typeof raw !== "object" || raw === null) return true;
      const ev = (raw as Record<string, unknown>).event;
      if (typeof ev !== "object" || ev === null) return true;
      const eventRecord = ev as Record<string, unknown>;
      if (eventRecord.kind !== "agent") return true;
      return typeof eventRecord.payload === "object" && eventRecord.payload !== null;
    })
    .map((event, index) => persistedEventToFrame(event, index));
  return { frames, available };
}

export async function readSessionHistory(
  baseUrl: string,
  sessionId: string,
  limit = 200,
): Promise<ConsoleSessionHistoryPage | null> {
  const trimmed = sessionId.trim();
  if (!trimmed) {
    return null;
  }
  const result = await rpc<unknown>(baseUrl, "mobkit/read_session_history", {
    session_id: trimmed,
    offset: 0,
    limit,
  });
  if (!result || typeof result !== "object") {
    return null;
  }
  return result as ConsoleSessionHistoryPage;
}

export async function sendInteraction(
  baseUrl: string,
  memberId: string,
  message: string
): Promise<{ sendResult: ConsoleSendMessageResult; frames: ConsoleFrame[] }> {
  // Open the SSE stream BEFORE sending. For fast members (cached/turn-driven)
  // the run_completed/interaction_complete events can fire before a
  // post-send subscription opens, causing the reply to be silently lost.
  const streamAbort = new AbortController();
  const streamResponsePromise = fetch(`${baseUrl}/interactions/stream`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ member_id: memberId }),
    signal: streamAbort.signal,
  });

  // Suppress the AbortError rejection that fires when we cancel the stream
  // on a failed send — it is always intentional and has no caller to handle it.
  void streamResponsePromise.catch(() => {});

  let sendResult: ConsoleSendMessageResult;
  try {
    sendResult = await sendMessage(baseUrl, memberId, message);
  } catch (err) {
    streamAbort.abort();
    throw err;
  }

  // The send succeeded — the turn was delivered. If the stream setup or read
  // fails, return empty frames rather than throwing. The caller must NOT roll
  // back the user message on a stream-only failure because the backend already
  // accepted the turn and retrying would create a duplicate.
  let frames: ConsoleFrame[];
  try {
    frames = await drainInteractionResponse(
      await streamResponsePromise,
      { sessionId: sendResult.session_id },
    );
  } catch {
    frames = [];
  }

  return { sendResult, frames };
}

export async function sendInteract(
  baseUrl: string,
  identity: string,
  content: string,
  origin: string,
): Promise<ConsoleInteractAccepted> {
  const accepted = await rpc<unknown>(baseUrl, "mobkit/interact", {
    identity,
    content,
    origin,
  });
  const normalized = normalizeConsoleInteractionAccepted(accepted);
  if (!normalized) {
    throw new Error("mobkit/interact returned an invalid acceptance payload");
  }
  return normalized;
}

export async function sendAddressedInteraction(
  baseUrl: string,
  target: ConsoleDockAddressedTarget,
  message: string,
  origin = "console",
): Promise<{ sendResult: ConsoleSendMessageResult | ConsoleInteractAccepted; frames: ConsoleFrame[] }> {
  if (target.addressingMode === "identity") {
    const identity = target.identity?.trim();
    if (!identity) {
      throw new Error("identity-addressed send requires target.identity");
    }

    const streamAbort = new AbortController();
    const streamResponsePromise = fetch(`${baseUrl}/console/identity/stream`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ identity }),
      signal: streamAbort.signal,
    });
    void streamResponsePromise.catch(() => {});

    let sendResult: ConsoleInteractAccepted;
    try {
      sendResult = await sendInteract(baseUrl, identity, message, origin);
    } catch (err) {
      streamAbort.abort();
      throw err;
    }

    let frames: ConsoleFrame[];
    try {
      frames = await drainInteractionResponse(
        await streamResponsePromise,
        { interactionId: sendResult.interaction_id },
      );
    } catch {
      frames = [];
    }

    return { sendResult, frames };
  }

  const memberId = target.memberId?.trim();
  if (!memberId) {
    throw new Error("member-addressed send requires target.memberId");
  }
  return sendInteraction(baseUrl, memberId, message);
}

export async function sendAddressedInteractionStreaming(
  baseUrl: string,
  target: ConsoleDockAddressedTarget,
  message: string,
  origin = "console",
  onFrame?: (frame: ConsoleFrame) => void,
): Promise<{ sendResult: ConsoleSendMessageResult | ConsoleInteractAccepted; frames: ConsoleFrame[] }> {
  if (target.addressingMode === "identity") {
    const identity = target.identity?.trim();
    if (!identity) {
      throw new Error("identity-addressed send requires target.identity");
    }

    const streamAbort = new AbortController();
    const streamResponsePromise = fetch(`${baseUrl}/console/identity/stream`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ identity }),
      signal: streamAbort.signal,
    });
    void streamResponsePromise.catch(() => {});

    let sendResult: ConsoleInteractAccepted;
    try {
      sendResult = await sendInteract(baseUrl, identity, message, origin);
    } catch (error) {
      streamAbort.abort();
      throw error;
    }

    let frames: ConsoleFrame[];
    try {
      frames = await streamFramesFromResponse(await streamResponsePromise, {
        correlation: { interactionId: sendResult.interaction_id },
        onFrame,
      });
    } catch {
      frames = [];
    }
    return { sendResult, frames };
  }

  const memberId = target.memberId?.trim();
  if (!memberId) {
    throw new Error("member-addressed send requires target.memberId");
  }

  const streamAbort = new AbortController();
  const streamResponsePromise = fetch(`${baseUrl}/interactions/stream`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ member_id: memberId }),
    signal: streamAbort.signal,
  });
  void streamResponsePromise.catch(() => {});

  let sendResult: ConsoleSendMessageResult;
  try {
    sendResult = await sendMessage(baseUrl, memberId, message);
  } catch (error) {
    streamAbort.abort();
    throw error;
  }

  let frames: ConsoleFrame[];
  try {
    frames = await streamFramesFromResponse(await streamResponsePromise, {
      correlation: { sessionId: sendResult.session_id },
      onFrame,
    });
  } catch {
    frames = [];
  }
  return { sendResult, frames };
}

export async function callConsoleRpc<T>(
  baseUrl: string,
  method: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  return rpc<T>(baseUrl, method, params);
}

export function subscribeConsoleEvents(
  baseUrl: string,
  path: string,
  onFrame: (frame: ConsoleFrame) => void,
  options?: { method?: "GET" | "POST"; body?: Record<string, unknown> },
): () => void {
  const controller = new AbortController();
  void (async () => {
    const response = await fetch(`${baseUrl}${path}`, {
      method: options?.method || "GET",
      headers: { "content-type": "application/json" },
      ...(options?.body ? { body: JSON.stringify(options.body) } : {}),
      signal: controller.signal,
    });
    await streamFramesFromResponse(response, { onFrame, stopOnTerminal: false });
  })().catch(() => {
    // The host polls /console/experience separately and can tolerate
    // best-effort stream failures during local development/example use.
  });

  return () => controller.abort();
}

export function subscribeIdentityEvents(
  baseUrl: string,
  identity: string,
  onFrame: (frame: ConsoleFrame) => void,
): () => void {
  return subscribeConsoleEvents(baseUrl, "/console/identity/stream", onFrame, {
    method: "POST",
    body: { identity },
  });
}
