import type { ConsoleFrame } from "../types";

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

export async function sendMessage(
  baseUrl: string,
  memberId: string,
  message: string
): Promise<void> {
  const response = await fetch(`${baseUrl}/rpc`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      jsonrpc: "2.0",
      id: `send_message:${Date.now()}`,
      method: "mobkit/send_message",
      params: { member_id: memberId, message },
    }),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(`send_message request failed ${response.status}: ${text}`);
  }

  const result = await response.json();
  if (result.error) {
    throw new Error(`send_message RPC error: ${result.error.message || JSON.stringify(result.error)}`);
  }
}
