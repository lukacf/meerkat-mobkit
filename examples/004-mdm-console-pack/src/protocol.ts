import { createHash, randomBytes, timingSafeEqual } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname } from "node:path";

export type TargetTransport = "legacy" | "control";

export type ScenarioTarget = {
  id: string;
  name: string;
  site: string;
  platform: string;
  transport: TargetTransport;
  port: number;
  labels?: Record<string, string>;
};

export type Scenario = {
  scenario_id: string;
  default_operator: string;
  api_listen_addr: string;
  console_expected_title: string;
  targets: ScenarioTarget[];
  links: Array<[string, string]>;
};

export type TargetRegistration = {
  target_id: string;
  name: string;
  site: string;
  platform: string;
  transport: TargetTransport;
  legacy_addr: string;
  control_addr: string;
  pubkey: string;
  labels: Record<string, string>;
  capabilities: Record<string, boolean>;
};

export type TargetRecord = TargetRegistration & {
  last_seen_ms: number;
  claim_state: "available" | "claimed";
  claimed_by?: string;
  lease_id?: string;
  lease_expires_at_ms?: number;
};

export type RemoteTurnRequest = {
  prompt: string;
  operator?: string;
  session_id?: string;
  handling_mode?: "queue" | "steer";
  model?: string;
};

export type RemoteTurnResult = {
  target_id: string;
  session_id: string;
  transport: TargetTransport;
  accepted: boolean;
  text: string;
  events: Array<Record<string, unknown>>;
};

export type ProcessHandle = {
  url: string;
  close(): Promise<void>;
};

export function parseHostPort(addr: string): { host: string; port: number } {
  const index = addr.lastIndexOf(":");
  if (index < 1) throw new Error(`invalid host:port address: ${addr}`);
  const host = addr.slice(0, index);
  const port = Number(addr.slice(index + 1));
  if (!Number.isInteger(port) || port <= 0) {
    throw new Error(`invalid port in address: ${addr}`);
  }
  return { host, port };
}

export function parseArgs(argv = process.argv.slice(2)): Record<string, string | boolean> {
  const result: Record<string, string | boolean> = {};
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (!arg.startsWith("--")) continue;
    const key = arg.slice(2);
    const next = argv[i + 1];
    if (next && !next.startsWith("--")) {
      result[key] = next;
      i += 1;
    } else {
      result[key] = true;
    }
  }
  return result;
}

export async function readJsonBody(req: NodeJS.ReadableStream): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = [];
  for await (const chunk of req) {
    chunks.push(Buffer.isBuffer(chunk) ? chunk : Buffer.from(String(chunk)));
  }
  if (chunks.length === 0) return {};
  const raw = Buffer.concat(chunks).toString("utf8");
  return JSON.parse(raw) as Record<string, unknown>;
}

export function sendJson(res: {
  statusCode: number;
  setHeader(name: string, value: string): void;
  end(body?: string): void;
}, status: number, body: unknown): void {
  res.statusCode = status;
  res.setHeader("content-type", "application/json; charset=utf-8");
  res.end(JSON.stringify(body, null, 2));
}

export function sendText(res: {
  statusCode: number;
  setHeader(name: string, value: string): void;
  end(body?: string): void;
}, status: number, body: string): void {
  res.statusCode = status;
  res.setHeader("content-type", "text/plain; charset=utf-8");
  res.end(body);
}

function secureEqual(left: string, right: string): boolean {
  const leftBytes = Buffer.from(left);
  const rightBytes = Buffer.from(right);
  if (leftBytes.length !== rightBytes.length) return false;
  return timingSafeEqual(leftBytes, rightBytes);
}

export function isAuthorized(req: {
  headers: Record<string, string | string[] | undefined>;
}, token?: string): boolean {
  if (!token) return true;
  const authorization = req.headers.authorization;
  const header = Array.isArray(authorization) ? authorization[0] : authorization;
  const bearer = header?.match(/^Bearer\s+(.+)$/i)?.[1];
  const fallback = req.headers["x-mdm-auth-token"];
  const headerToken = bearer ?? (Array.isArray(fallback) ? fallback[0] : fallback);
  return typeof headerToken === "string" && secureEqual(headerToken, token);
}

export function authHeaders(token?: string): Record<string, string> {
  return token ? { authorization: `Bearer ${token}` } : {};
}

export async function postJson<T>(
  url: string,
  body: unknown,
  timeoutMs = 10_000,
  authToken?: string,
): Promise<T> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json", ...authHeaders(authToken) },
      body: JSON.stringify(body),
      signal: controller.signal,
    });
    const text = await response.text();
    if (!response.ok) {
      throw new Error(`POST ${url} returned ${response.status}: ${text}`);
    }
    return (text ? JSON.parse(text) : null) as T;
  } finally {
    clearTimeout(timer);
  }
}

export async function getJson<T>(url: string, timeoutMs = 10_000, authToken?: string): Promise<T> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const response = await fetch(url, { headers: authHeaders(authToken), signal: controller.signal });
    const text = await response.text();
    if (!response.ok) {
      throw new Error(`GET ${url} returned ${response.status}: ${text}`);
    }
    return (text ? JSON.parse(text) : null) as T;
  } finally {
    clearTimeout(timer);
  }
}

export async function waitFor(
  label: string,
  fn: () => Promise<boolean>,
  timeoutMs = 20_000,
): Promise<void> {
  const start = Date.now();
  let lastError: unknown = null;
  while (Date.now() - start < timeoutMs) {
    try {
      if (await fn()) return;
    } catch (error) {
      lastError = error;
    }
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  const suffix = lastError instanceof Error ? `: ${lastError.message}` : "";
  throw new Error(`timed out waiting for ${label}${suffix}`);
}

export function stableSessionId(targetId: string): string {
  const hash = createHash("sha256").update(targetId).digest("hex").slice(0, 12);
  return `mdm-${hash}`;
}

export function ensureTargetKeypair(path: string): { publicKey: string } {
  if (existsSync(path)) {
    const existing = JSON.parse(readFileSync(path, "utf8")) as { publicKey: string };
    const raw = Buffer.from(existing.publicKey.replace(/^ed25519:/, ""), "base64");
    if (raw.length === 32) return existing;
  }
  mkdirSync(dirname(path), { recursive: true });
  const publicKey = randomBytes(32).toString("base64");
  const payload = { publicKey: `ed25519:${publicKey}` };
  writeFileSync(path, JSON.stringify(payload, null, 2));
  return payload;
}

export function targetLabels(target: ScenarioTarget | TargetRecord): Record<string, string> {
  return {
    ...(target.labels ?? {}),
    site: target.site,
    platform: target.platform,
    transport: target.transport,
  };
}
