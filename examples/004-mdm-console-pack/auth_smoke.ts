import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const examplesRoot = resolve(here, "..");
const authToken = "mdm-auth-smoke-kennel";
const targetToken = "mdm-auth-smoke-target";

function waitForLine(child: ReturnType<typeof spawn>, pattern: RegExp, timeoutMs = 120_000): Promise<string> {
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error(`timed out waiting for ${pattern}`)), timeoutMs);
    const onData = (chunk: Buffer) => {
      const text = chunk.toString();
      process.stdout.write(text);
      const match = text.match(pattern);
      if (match) {
        clearTimeout(timer);
        child.stdout?.off("data", onData);
        resolve(match[1]);
      }
    };
    child.stdout?.on("data", onData);
    child.stderr?.on("data", (chunk) => process.stderr.write(chunk.toString()));
  });
}

async function jsonFetch(url: string, init: RequestInit = {}): Promise<Response> {
  return fetch(url, {
    ...init,
    headers: {
      ...(init.headers ?? {}),
      "content-type": "application/json",
    },
  });
}

const child = spawn(
  process.execPath,
  [
    join(examplesRoot, "node_modules/tsx/dist/cli.mjs"),
    join(here, "run.ts"),
    "--api-only",
    "--wait",
    "--spawn-targets",
    "--skip-build",
    "--api-listen",
    "127.0.0.1:5789",
    "--require-auth",
  ],
  {
    cwd: examplesRoot,
    env: {
      ...process.env,
      MDM_AUTH_TOKEN: authToken,
      MDM_TARGET_AUTH_TOKEN: targetToken,
    },
    stdio: ["ignore", "pipe", "pipe"],
  },
);

try {
  const apiUrl = await waitForLine(child, /\[mdm\] api: (http:\/\/[^\s]+)/);
  const unauthorized = await jsonFetch(`${apiUrl}/api/targets`);
  if (unauthorized.status !== 401) {
    throw new Error(`expected unauthenticated /api/targets to return 401, got ${unauthorized.status}`);
  }

  const authorized = await jsonFetch(`${apiUrl}/api/targets`, {
    headers: { authorization: `Bearer ${authToken}` },
  });
  if (!authorized.ok) throw new Error(`authorized target list failed: ${authorized.status}`);
  const body = (await authorized.json()) as { targets: Array<{ target_id: string }> };
  if (body.targets.length < 2) throw new Error(`expected at least two targets, got ${body.targets.length}`);

  const turn = await jsonFetch(`${apiUrl}/api/targets/target-b/turn`, {
    method: "POST",
    headers: { authorization: `Bearer ${authToken}` },
    body: JSON.stringify({ operator: "auth-smoke", prompt: "shell: echo MOBKIT_MDM_AUTH_SMOKE" }),
  });
  const turnBody = (await turn.json()) as { text?: string };
  if (!turn.ok || !turnBody.text?.includes("MOBKIT_MDM_AUTH_SMOKE")) {
    throw new Error(`authenticated remote turn failed: ${JSON.stringify(turnBody)}`);
  }

  const statePath = join(here, ".state", "kennel-state.json");
  if (!existsSync(statePath)) throw new Error("kennel-state.json was not persisted");
  console.log("[mdm-auth-smoke] ok");
} finally {
  child.kill("SIGTERM");
}
