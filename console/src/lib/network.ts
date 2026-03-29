import type { ConsoleFrame, ConsoleSendMessageResult } from "../types";

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

    frames.push({ id, event, data });
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

/**
 * Scan complete SSE blocks (delimited by double-newline) for a terminal
 * event: line.  The last block is skipped because it may be incomplete.
 * If `sessionId` is supplied, only a terminal event whose JSON data carries
 * a matching `session_id` field stops the stream; terminals from other
 * sessions (concurrent turns, other clients) are ignored so they cannot
 * prematurely satisfy the stop condition.
 */
function hasMatchingTerminalEvent(rawText: string, sessionId?: string): boolean {
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
    if (!sessionId) return true;
    try {
      const data = JSON.parse(dataLines.join("\n")) as Record<string, unknown>;
      if (data.session_id === sessionId) return true;
    } catch {
      // unparseable data — treat as unmatched
    }
  }
  return false;
}

async function drainInteractionResponse(
  response: Response,
  sessionId?: string
): Promise<ConsoleFrame[]> {
  if (!response.ok) {
    const text = await response.text();
    throw new Error(`interaction stream request failed ${response.status}: ${text}`);
  }

  if (!response.body || typeof response.body.getReader !== "function") {
    return parseSseFrames(await response.text());
  }

  const reader = response.body.getReader();
  const decoder = new TextDecoder();
  let rawText = "";

  try {
    while (!hasMatchingTerminalEvent(rawText, sessionId)) {
      const { value, done } = await reader.read();
      if (done) {
        break;
      }
      rawText += decoder.decode(value, { stream: true });
      if (rawText.length > 131_072) {
        break;
      }
    }
    rawText += decoder.decode();
  } finally {
    try {
      await reader.cancel();
    } catch {
      // Reader cancellation is best-effort only.
    }
  }

  const frames = parseSseFrames(rawText);
  if (!sessionId) return frames;

  // Filter to frames that either belong to this session or carry no session_id
  // (infrastructure frames like "subscribed" belong to no turn).
  return frames.filter((frame) => {
    const data = frame.data as Record<string, unknown> | null;
    if (data === null || typeof data !== "object") return false;
    if ("session_id" in data) return data.session_id === sessionId;
    return true;
  });
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

function persistedEventToFrame(raw: unknown, index: number): ConsoleFrame {
  const record = typeof raw === "object" && raw !== null
    ? raw as Record<string, unknown>
    : {};
  const event = typeof record.event === "object" && record.event !== null
    ? record.event as Record<string, unknown>
    : {};

  // UnifiedEvent is serde(tag = "kind", rename_all = "snake_case"), so the
  // wire format is {"kind": "agent", ...} / {"kind": "module", ...}.
  if (event.kind === "agent") {
    return {
      id: String(record.id ?? `event:${index}`),
      event: String(event.event_type ?? "agent_event"),
      data: event,
    };
  }

  if (event.kind === "module") {
    return {
      id: String(record.id ?? `event:${index}`),
      event: String(event.event_type ?? "module_event"),
      data: (event.payload as unknown) ?? event,
    };
  }

  return {
    id: String(record.id ?? `event:${index}`),
    event: String(record.type ?? "event"),
    data: raw,
  };
}

export async function queryEvents(
  baseUrl: string,
  memberId: string,
  limit = 40
): Promise<ConsoleFrame[]> {
  // Do not filter by member_id: module events (the only ones with displayable
  // payloads) are persisted with member_id: null, so a member_id filter would
  // exclude them on the server side.
  const result = await rpc<unknown>(baseUrl, "mobkit/query_events", {
    limit,
  });

  if (
    typeof result === "object" &&
    result !== null &&
    (result as Record<string, unknown>).status === "no_event_log_configured"
  ) {
    return [];
  }

  if (!Array.isArray(result)) {
    return [];
  }

  // UnifiedEvent::Agent stores only agent_id + event_type — the actual
  // text/payload is not persisted. Skip agent-kind rows so history only
  // includes events that carry displayable content (module events with payload).
  return result
    .filter((raw) => {
      if (typeof raw !== "object" || raw === null) return true;
      const ev = (raw as Record<string, unknown>).event;
      return !(
        typeof ev === "object" &&
        ev !== null &&
        (ev as Record<string, unknown>).kind === "agent"
      );
    })
    .map((event, index) => persistedEventToFrame(event, index));
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
      sendResult.session_id,
    );
  } catch {
    frames = [];
  }

  return { sendResult, frames };
}
