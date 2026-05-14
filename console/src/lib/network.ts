import {
  normalizeConsoleInteractionRejectedError,
  normalizeReplayUnavailableError,
} from "@console-core";
import type {
  ConsoleFrame,
  ConsoleTimelineAccepted,
  ConsoleTimelinePage,
  ConsoleReplayUnavailablePayload,
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
  sourceKind?: string;
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
      sourceKind: frame.sourceKind,
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
      sourceKind: normalized.sourceKind,
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

export async function callConsoleRpc<T>(
  baseUrl: string,
  method: string,
  params: Record<string, unknown> = {},
): Promise<T> {
  return rpc<T>(baseUrl, method, params);
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
