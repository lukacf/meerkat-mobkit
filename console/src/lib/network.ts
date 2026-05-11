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
  ConsoleTimelineAccepted,
  ConsoleTimelinePage,
  ConsoleReplayUnavailablePayload,
  ConsoleSessionHistoryPage,
  ConsoleSendMessageResult,
  ConsoleGatewayInteractionRejectedError,
} from "../types";

function unwrapConsoleEnvelope(
  eventName: string,
  data: unknown,
): {
  id?: string;
  event?: string;
  identity?: string;
  interactionId?: string;
  timestampMs?: number;
  cursor?: string;
  runtimeKey?: string;
  sessionId?: string;
  status?: string;
  frameVersion?: number;
  updatedAtMs?: number;
  turnId?: string;
  runId?: string;
  data: unknown;
} {
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
  if (typeof record.type === "string" && "frame" in record) {
    const frame = timelineFrameToConsoleFrame(record.frame);
    const isUpdateEnvelope = eventName === "frame_updated";
    return {
      id: frame.id,
      event: isUpdateEnvelope ? "frame_updated" : frame.event,
      identity: frame.identity,
      interactionId: frame.interactionId,
      timestampMs: frame.timestampMs,
      cursor: frame.cursor,
      runtimeKey: frame.runtimeKey,
      sessionId: frame.sessionId,
      status: frame.status,
      frameVersion: frame.frameVersion,
      updatedAtMs: frame.updatedAtMs,
      turnId: frame.turnId,
      runId: frame.runId,
      data: isUpdateEnvelope
        ? frame.event === "frame_updated"
          ? frame.data
          : { frame }
        : frame.data,
    };
  }
  return { data };
}

function timelineFrameToConsoleFrame(raw: unknown): ConsoleFrame {
  if (!raw || typeof raw !== "object") {
    return { id: "", event: "event", data: raw };
  }
  const record = raw as Record<string, unknown>;
  const cursor = typeof record.cursor === "string" ? record.cursor : undefined;
  const payload = "payload" in record ? record.payload : record;
  const source = record.source && typeof record.source === "object"
    ? record.source as Record<string, unknown>
    : null;
  if (
    record.kind === "frame_updated" &&
    payload &&
    typeof payload === "object" &&
    "frame" in payload
  ) {
    const updated = timelineFrameToConsoleFrame((payload as Record<string, unknown>).frame);
    return {
      id: String(record.id || cursor || ""),
      event: "frame_updated",
      identity: typeof record.identity === "string" ? record.identity : updated.identity,
      interactionId:
        typeof record.interaction_id === "string" ? record.interaction_id : updated.interactionId,
      timestampMs: typeof record.timestamp_ms === "number" ? record.timestamp_ms : undefined,
      cursor,
      runtimeKey: typeof record.runtime_key === "string" ? record.runtime_key : updated.runtimeKey,
      sessionId: typeof record.session_id === "string" ? record.session_id : updated.sessionId,
      status: typeof record.status === "string" ? record.status : updated.status,
      sourceKind: source && typeof source.kind === "string" ? source.kind : updated.sourceKind,
      frameVersion:
        typeof record.frame_version === "number" ? record.frame_version : updated.frameVersion,
      updatedAtMs:
        typeof record.updated_at_ms === "number" ? record.updated_at_ms : updated.updatedAtMs,
      turnId: typeof record.turn_id === "string" ? record.turn_id : updated.turnId,
      runId: typeof record.run_id === "string" ? record.run_id : updated.runId,
      data: { frame: updated },
    };
  }
  return {
    id: String(record.id || cursor || ""),
    event: String(record.kind || "event"),
    identity: typeof record.identity === "string" ? record.identity : undefined,
    interactionId: typeof record.interaction_id === "string" ? record.interaction_id : undefined,
    timestampMs: typeof record.timestamp_ms === "number" ? record.timestamp_ms : undefined,
    cursor,
    runtimeKey: typeof record.runtime_key === "string" ? record.runtime_key : undefined,
    sessionId: typeof record.session_id === "string" ? record.session_id : undefined,
    status: typeof record.status === "string" ? record.status : undefined,
    sourceKind: source && typeof source.kind === "string" ? source.kind : undefined,
    frameVersion: typeof record.frame_version === "number" ? record.frame_version : undefined,
    updatedAtMs: typeof record.updated_at_ms === "number" ? record.updated_at_ms : undefined,
    turnId: typeof record.turn_id === "string" ? record.turn_id : undefined,
    runId: typeof record.run_id === "string" ? record.run_id : undefined,
    data: payload,
  };
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
      cursor: normalized.cursor,
      runtimeKey: normalized.runtimeKey,
      sessionId: normalized.sessionId,
      status: normalized.status,
      frameVersion: normalized.frameVersion,
      updatedAtMs: normalized.updatedAtMs,
      turnId: normalized.turnId,
      runId: normalized.runId,
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
  message: string,
  handlingMode: "queue" | "steer" = "queue",
): Promise<ConsoleSendMessageResult> {
  return rpc<ConsoleSendMessageResult>(baseUrl, "mobkit/send_message", {
    member_id: memberId,
    message,
    handling_mode: handlingMode,
  });
}

export async function sendMessageMultipart(
  baseUrl: string,
  memberId: string,
  message: string,
  attachments: File[],
  handlingMode: "queue" | "steer" = "queue",
): Promise<ConsoleSendMessageResult> {
  const content: Array<Record<string, unknown>> = [];
  if (message.trim()) {
    content.push({ type: "text", text: message });
  }
  const form = new FormData();
  attachments.forEach((file, index) => {
    const uploadId = `upload-${Date.now().toString(36)}-${index}`;
    content.push({
      type: "image_upload",
      upload_id: uploadId,
      media_type: file.type || "application/octet-stream",
      alt: file.name,
    });
    form.append(`file:${uploadId}`, file, file.name);
  });
  form.append("payload", JSON.stringify({
    jsonrpc: "2.0",
    id: `mobkit/send_message:${Date.now()}`,
    method: "mobkit/send_message",
    params: {
      member_id: memberId,
      content,
      handling_mode: handlingMode,
    },
  }));

  const response = await fetch(`${baseUrl}/console/rpc/multipart`, {
    method: "POST",
    body: form,
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`mobkit/send_message multipart failed ${response.status}: ${text}`);
  }
  const result = await response.json();
  if (result.error) {
    throw new Error(`mobkit/send_message RPC error: ${result.error.message || JSON.stringify(result.error)}`);
  }
  return result.result as ConsoleSendMessageResult;
}

export async function sendConsoleMultipart(
  baseUrl: string,
  identity: string,
  message: string,
  attachments: File[],
  origin: string,
  idempotencyKey: string,
  handlingMode: "queue" | "steer" = "queue",
): Promise<ConsoleTimelineAccepted> {
  const content: Array<Record<string, unknown>> = [];
  if (message.trim()) {
    content.push({ type: "text", text: message });
  }
  const form = new FormData();
  attachments.forEach((file, index) => {
    const uploadId = `upload-${Date.now().toString(36)}-${index}`;
    content.push({
      type: "image_upload",
      upload_id: uploadId,
      media_type: file.type || "application/octet-stream",
      alt: file.name,
    });
    form.append(`file:${uploadId}`, file, file.name);
  });
  form.append("payload", JSON.stringify({
    jsonrpc: "2.0",
    id: `mobkit/console/send:${Date.now()}`,
    method: "mobkit/console/send",
    params: {
      identity,
      content,
      origin,
      idempotency_key: idempotencyKey,
      handling_mode: handlingMode,
    },
  }));

  const response = await fetch(`${baseUrl}/console/rpc/multipart`, {
    method: "POST",
    body: form,
  });
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`mobkit/console/send multipart failed ${response.status}: ${text}`);
  }
  const result = await response.json();
  if (result.error) {
    throw new Error(`mobkit/console/send RPC error: ${result.error.message || JSON.stringify(result.error)}`);
  }
  return normalizeConsoleTimelineAccepted(result.result, identity);
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

export async function queryTimeline(
  baseUrl: string,
  target: { identity?: string; conversationId?: string; after?: string },
  limit = 400,
): Promise<ConsoleTimelinePage> {
  const result = await rpc<unknown>(baseUrl, "mobkit/console/query_timeline", {
    limit,
    ...(target.identity?.trim() ? { identity: target.identity.trim() } : {}),
    ...(target.conversationId?.trim() ? { conversation_id: target.conversationId.trim() } : {}),
    ...(target.after?.trim() ? { after: target.after.trim() } : {}),
  });
  if (!result || typeof result !== "object") {
    return { frames: [], available: false };
  }
  const record = result as Record<string, unknown>;
  const rawFrames = Array.isArray(record.frames) ? record.frames : [];
  return {
    frames: rawFrames.map(timelineFrameToConsoleFrame),
    nextCursor: typeof record.next_cursor === "string" ? record.next_cursor : undefined,
    available: true,
  };
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
  handlingMode: "queue" | "steer" = "queue",
): Promise<ConsoleInteractAccepted> {
  const accepted = await rpc<unknown>(baseUrl, "mobkit/interact", {
    identity,
    content,
    origin,
    handling_mode: handlingMode,
  });
  const normalized = normalizeConsoleInteractionAccepted(accepted);
  if (!normalized) {
    throw new Error("mobkit/interact returned an invalid acceptance payload");
  }
  return normalized;
}

export async function sendConsole(
  baseUrl: string,
  identity: string,
  content: string | Array<Record<string, unknown>>,
  origin: string,
  idempotencyKey: string,
  handlingMode: "queue" | "steer" = "queue",
): Promise<ConsoleTimelineAccepted> {
  const accepted = await rpc<unknown>(baseUrl, "mobkit/console/send", {
    identity,
    content,
    origin,
    idempotency_key: idempotencyKey,
    handling_mode: handlingMode,
  });
  if (!accepted || typeof accepted !== "object") {
    throw new Error("mobkit/console/send returned an invalid acceptance payload");
  }
  const record = accepted as Record<string, unknown>;
  return normalizeConsoleTimelineAccepted(record, identity);
}

function normalizeConsoleTimelineAccepted(
  accepted: unknown,
  fallbackIdentity: string,
): ConsoleTimelineAccepted {
  const record = accepted && typeof accepted === "object" ? accepted as Record<string, unknown> : {};
  return {
    interaction_id: String(record.interaction_id || ""),
    identity: String(record.identity || fallbackIdentity),
    conversation_id: typeof record.conversation_id === "string" ? record.conversation_id : undefined,
    session_id: typeof record.session_id === "string" ? record.session_id : undefined,
    input_frame_id: typeof record.input_frame_id === "string" ? record.input_frame_id : undefined,
    cursor: typeof record.cursor === "string" ? record.cursor : undefined,
    status: typeof record.status === "string" ? record.status : undefined,
  };
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

function timelineStreamPath(target: { identity?: string; conversationId?: string; after?: string }): string {
  const params = new URLSearchParams();
  if (target.identity?.trim()) params.set("identity", target.identity.trim());
  if (target.conversationId?.trim()) params.set("conversation_id", target.conversationId.trim());
  if (target.after?.trim()) params.set("after", target.after.trim());
  return `/console/timeline/stream${params.size > 0 ? `?${params.toString()}` : ""}`;
}

function cursorFromTimelineFrame(frame: ConsoleFrame): string | undefined {
  const cursor = frame.cursor?.trim();
  if (cursor) return cursor;
  if (frame.event === "snapshot_complete") {
    const id = frame.id?.trim();
    if (id?.startsWith("console:")) return id;
  }
  return undefined;
}

function replayUnavailableFrame(error: unknown): ConsoleFrame {
  const replayError = (error as Error & { replayError?: ConsoleReplayUnavailablePayload }).replayError;
  return {
    id: `replay_unavailable:${Date.now()}`,
    event: "replay_unavailable",
    data: replayError || {
      message: error instanceof Error ? error.message : String(error),
    },
  };
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

export function subscribeTimelineEvents(
  baseUrl: string,
  target: { identity?: string; conversationId?: string; after?: string },
  onFrame: (frame: ConsoleFrame) => void,
): () => void {
  let stopped = false;
  let controller: AbortController | null = null;
  let after = target.after?.trim() || undefined;

  void (async () => {
    let retryDelayMs = 250;
    while (!stopped) {
      controller = new AbortController();
      try {
        await streamFramesFromResponse(
          await fetch(`${baseUrl}${timelineStreamPath({ ...target, after })}`, {
            method: "GET",
            headers: { "content-type": "application/json" },
            signal: controller.signal,
          }),
          {
            stopOnTerminal: false,
            onFrame: (frame) => {
              const nextCursor = cursorFromTimelineFrame(frame);
              if (nextCursor) {
                after = nextCursor;
              }
              onFrame(frame);
            },
          },
        );
        retryDelayMs = 250;
      } catch (error) {
        if (stopped || controller.signal.aborted) {
          break;
        }
        onFrame(replayUnavailableFrame(error));
      }
      if (!stopped) {
        await sleep(retryDelayMs);
        retryDelayMs = Math.min(retryDelayMs * 2, 2_000);
      }
    }
  })();

  return () => {
    stopped = true;
    controller?.abort();
  };
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
