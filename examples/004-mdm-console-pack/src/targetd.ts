import { exec } from "node:child_process";
import { createServer, type Server } from "node:http";
import { mkdirSync } from "node:fs";
import { join } from "node:path";
import { promisify } from "node:util";

import {
  ensureTargetKeypair,
  isAuthorized,
  parseArgs,
  parseHostPort,
  postJson,
  readJsonBody,
  sendJson,
  stableSessionId,
  type ProcessHandle,
  type RemoteTurnRequest,
  type RemoteTurnResult,
  type TargetRegistration,
  type TargetTransport,
} from "./protocol.js";

const execAsync = promisify(exec);

export type TargetDaemonOptions = {
  id: string;
  name: string;
  site: string;
  platform: string;
  transport: TargetTransport;
  listen: string;
  advertiseUrl?: string;
  kennelUrl?: string;
  kennelAuthToken?: string;
  controlAuthToken?: string;
  stateDir: string;
  allowShell?: boolean;
  labels?: Record<string, string>;
};

type TargetState = {
  model: string;
  generation: number;
  interrupted: boolean;
};

function registrationFor(options: TargetDaemonOptions, url: string, pubkey: string): TargetRegistration {
  return {
    target_id: options.id,
    name: options.name,
    site: options.site,
    platform: options.platform,
    transport: options.transport,
    legacy_addr: `${url}/legacy`,
    control_addr: `${url}/control`,
    pubkey,
    labels: options.labels ?? {},
    capabilities: {
      legacy_turn: true,
      control_turn: options.transport === "control",
      shell: Boolean(options.allowShell),
      respawn: true,
      model_select: true,
      peer_pubkey: true,
    },
  };
}

async function executePrompt(
  options: TargetDaemonOptions,
  state: TargetState,
  request: RemoteTurnRequest,
  route: TargetTransport,
): Promise<RemoteTurnResult> {
  const prompt = request.prompt.trim();
  const sessionId = request.session_id || stableSessionId(options.id);
  const events: Array<Record<string, unknown>> = [
    { type: "run_started", target_id: options.id, route, session_id: sessionId },
  ];
  let text: string;

  const shellMatch = prompt.match(/^(?:shell|run):\s*(.+)$/is);
  if (shellMatch && options.allowShell) {
    const command = shellMatch[1].trim();
    events.push({ type: "tool_call_requested", name: "shell", args: { command } });
    const { stdout, stderr } = await execAsync(command, {
      timeout: 8_000,
      maxBuffer: 64 * 1024,
      env: process.env,
    });
    const output = `${stdout}${stderr ? `\n${stderr}` : ""}`.trim();
    events.push({
      type: "tool_execution_completed",
      name: "shell",
      is_error: false,
      result: output,
    });
    text = output || "(command completed with no output)";
  } else {
    text = [
      `${options.name} accepted remote ${route} turn.`,
      `site=${options.site}`,
      `platform=${options.platform}`,
      `model=${request.model ?? state.model}`,
      `prompt=${prompt}`,
    ].join(" | ");
  }

  events.push({ type: "text_complete", content: text });
  events.push({ type: "run_completed", target_id: options.id, session_id: sessionId });
  return {
    target_id: options.id,
    session_id: sessionId,
    transport: route,
    accepted: true,
    text,
    events,
  };
}

export async function startTargetDaemon(options: TargetDaemonOptions): Promise<ProcessHandle> {
  mkdirSync(options.stateDir, { recursive: true });
  const keypair = ensureTargetKeypair(join(options.stateDir, "identity.json"));
  const state: TargetState = {
    model: "demo-target-model",
    generation: 0,
    interrupted: false,
  };
  const { host, port } = parseHostPort(options.listen);
  let heartbeat: NodeJS.Timeout | null = null;
  let registered = false;

  const server: Server = createServer(async (req, res) => {
    try {
      const pathname = new URL(req.url ?? "/", "http://targetd.local").pathname;
      const baseUrl = options.advertiseUrl ?? `http://${host === "0.0.0.0" ? "127.0.0.1" : host}:${port}`;
      if (pathname !== "/health" && !isAuthorized(req, options.controlAuthToken)) {
        sendJson(res, 401, { error: "unauthorized" });
        return;
      }
      if (req.method === "GET" && pathname === "/health") {
        sendJson(res, 200, { ok: true, target_id: options.id, generation: state.generation });
        return;
      }
      if (req.method === "GET" && pathname === "/mdm/info") {
        sendJson(res, 200, registrationFor(options, baseUrl, keypair.publicKey));
        return;
      }
      if (req.method === "GET" && pathname === "/control/sessions") {
        sendJson(res, 200, {
          sessions: [{ session_id: stableSessionId(options.id), state: "idle" }],
        });
        return;
      }
      if (req.method === "GET" && pathname === "/control/peer_pubkey") {
        sendJson(res, 200, { pubkey: keypair.publicKey });
        return;
      }
      if (req.method === "POST" && (pathname === "/legacy/turn" || pathname === "/control/turn")) {
        const request = (await readJsonBody(req)) as RemoteTurnRequest;
        const route = pathname.startsWith("/legacy") ? "legacy" : "control";
        sendJson(res, 200, await executePrompt(options, state, request, route));
        return;
      }
      if (req.method === "POST" && pathname === "/control/interrupt") {
        state.interrupted = true;
        sendJson(res, 200, { accepted: true, target_id: options.id });
        return;
      }
      if (req.method === "POST" && pathname === "/control/respawn") {
        state.generation += 1;
        state.interrupted = false;
        sendJson(res, 200, {
          accepted: true,
          target_id: options.id,
          generation: state.generation,
          session_id: stableSessionId(`${options.id}:${state.generation}`),
        });
        return;
      }
      if (req.method === "POST" && pathname === "/control/model") {
        const body = await readJsonBody(req);
        state.model = String(body.model ?? state.model);
        sendJson(res, 200, { accepted: true, target_id: options.id, model: state.model });
        return;
      }
      sendJson(res, 404, { error: "not_found", path: pathname });
    } catch (error) {
      sendJson(res, 500, {
        error: "targetd_error",
        message: error instanceof Error ? error.message : String(error),
      });
    }
  });

  await new Promise<void>((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, host, () => {
      server.off("error", reject);
      resolve();
    });
  });

  const listenUrl = `http://${host === "0.0.0.0" ? "127.0.0.1" : host}:${port}`;
  const advertiseUrl = options.advertiseUrl ?? listenUrl;
  if (options.kennelUrl) {
    const register = async () => {
      await postJson(
        `${options.kennelUrl}/api/register`,
        registrationFor(options, advertiseUrl, keypair.publicKey),
        10_000,
        options.kennelAuthToken,
      );
      registered = true;
    };
    register().catch(() => {});
    heartbeat = setInterval(() => {
      if (!registered) {
        register().catch(() => {});
        return;
      }
      postJson(`${options.kennelUrl}/api/heartbeat`, { target_id: options.id }, 10_000, options.kennelAuthToken).catch(() => {
        registered = false;
      });
    }, 2_000);
  }

  return {
    url: listenUrl,
    async close() {
      if (heartbeat) clearInterval(heartbeat);
      await new Promise<void>((resolve) => server.close(() => resolve()));
    },
  };
}

async function main() {
  const args = parseArgs();
  const id = String(args.id ?? args.name ?? "target");
  const listen = String(args.listen ?? "127.0.0.1:5791");
  const handle = await startTargetDaemon({
    id,
    name: String(args.name ?? id),
    site: String(args.site ?? "local"),
    platform: String(args.platform ?? process.platform),
    transport: (String(args.transport ?? "control") as TargetTransport),
    listen,
    advertiseUrl: typeof args["advertise-url"] === "string" ? args["advertise-url"] : undefined,
    kennelUrl: typeof args.kennel === "string" ? args.kennel : undefined,
    kennelAuthToken: typeof args["kennel-auth-token"] === "string" ? args["kennel-auth-token"] : process.env.MDM_AUTH_TOKEN,
    controlAuthToken: typeof args["control-auth-token"] === "string" ? args["control-auth-token"] : process.env.MDM_TARGET_AUTH_TOKEN,
    stateDir: String(args["state-dir"] ?? `.target-state/${id}`),
    allowShell: Boolean(args["allow-shell"]),
  });
  console.log(`[mdm-targetd] ${id} listening at ${handle.url}`);
  await new Promise<void>((resolve) => {
    process.once("SIGINT", resolve);
    process.once("SIGTERM", resolve);
  });
  await handle.close();
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    console.error(error instanceof Error ? error.stack : error);
    process.exitCode = 1;
  });
}
