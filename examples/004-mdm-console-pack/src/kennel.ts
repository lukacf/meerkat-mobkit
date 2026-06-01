import {
  createServer as createHttpServer,
  type IncomingMessage,
  type Server,
  type ServerResponse,
} from "node:http";
import { createServer as createHttpsServer } from "node:https";
import { randomUUID } from "node:crypto";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import {
  getJson,
  isAuthorized,
  parseHostPort,
  postJson,
  readJsonBody,
  sendJson,
  sendText,
  targetLabels,
  type ProcessHandle,
  type RemoteTurnRequest,
  type RemoteTurnResult,
  type TargetRecord,
  type TargetRegistration,
  type TargetTransport,
} from "./protocol.js";

export type KennelOptions = {
  listen: string;
  stateDir: string;
  defaultOperator: string;
  authToken?: string;
  targetAuthToken?: string;
  tlsCertPath?: string;
  tlsKeyPath?: string;
};

type KennelState = {
  targets: TargetRecord[];
};

export class MdmKennel {
  readonly targets = new Map<string, TargetRecord>();
  private server: Server | null = null;
  private readonly options: KennelOptions;

  constructor(options: KennelOptions) {
    this.options = options;
  }

  async start(): Promise<ProcessHandle> {
    mkdirSync(this.options.stateDir, { recursive: true });
    this.loadState();
    this.writeContacts();
    const { host, port } = parseHostPort(this.options.listen);
    const handler = async (req: IncomingMessage, res: ServerResponse) => {
      try {
        const url = new URL(req.url ?? "/", `http://${this.options.listen}`);
        const path = url.pathname;
        if (req.method === "GET" && path === "/health") {
          sendJson(res, 200, { ok: true, targets: this.targets.size });
          return;
        }
        if (path.startsWith("/api/") && !isAuthorized(req, this.options.authToken)) {
          sendJson(res, 401, { error: "unauthorized" });
          return;
        }
        if (req.method === "GET" && path === "/api/targets") {
          sendJson(res, 200, { targets: this.listTargets() });
          return;
        }
        if (req.method === "GET" && path === "/api/contacts.toml") {
          sendText(res, 200, this.contactsToml());
          return;
        }
        if (req.method === "POST" && path === "/api/register") {
          const body = (await readJsonBody(req)) as TargetRegistration;
          const record = this.register(body);
          sendJson(res, 200, { accepted: true, target: record });
          return;
        }
        if (req.method === "POST" && path === "/api/heartbeat") {
          const body = await readJsonBody(req);
          const id = String(body.target_id ?? "");
          const target = this.targets.get(id);
          if (target) {
            target.last_seen_ms = Date.now();
            this.writeState();
          }
          sendJson(res, target ? 200 : 404, { accepted: Boolean(target), target_id: id });
          return;
        }
        const targetAction = path.match(/^\/api\/targets\/([^/]+)\/([^/]+)$/);
        if (targetAction && req.method === "POST") {
          const [, targetId, action] = targetAction;
          const body = await readJsonBody(req);
          const result = await this.handleTargetAction(targetId, action, body);
          sendJson(res, 200, result);
          return;
        }
        const targetRead = path.match(/^\/api\/targets\/([^/]+)$/);
        if (targetRead && req.method === "GET") {
          const target = this.targets.get(targetRead[1]);
          sendJson(res, target ? 200 : 404, target ? { target } : { error: "unknown_target" });
          return;
        }
        sendJson(res, 404, { error: "not_found", path });
      } catch (error) {
        sendJson(res, 500, {
          error: "kennel_error",
          message: error instanceof Error ? error.message : String(error),
        });
      }
    };

    if (this.options.tlsCertPath || this.options.tlsKeyPath) {
      if (!this.options.tlsCertPath || !this.options.tlsKeyPath) {
        throw new Error("both tlsCertPath and tlsKeyPath are required for HTTPS");
      }
      this.server = createHttpsServer({
        cert: readFileSync(this.options.tlsCertPath),
        key: readFileSync(this.options.tlsKeyPath),
      }, handler);
    } else {
      this.server = createHttpServer(handler);
    }

    await new Promise<void>((resolve, reject) => {
      this.server?.once("error", reject);
      this.server?.listen(port, host, () => {
        this.server?.off("error", reject);
        resolve();
      });
    });
    const scheme = this.options.tlsCertPath ? "https" : "http";
    const url = `${scheme}://${host === "0.0.0.0" ? "127.0.0.1" : host}:${port}`;
    return {
      url,
      close: async () => {
        await new Promise<void>((resolve) => this.server?.close(() => resolve()));
      },
    };
  }

  register(registration: TargetRegistration): TargetRecord {
    const previous = this.targets.get(registration.target_id);
    const record: TargetRecord = {
      ...registration,
      labels: targetLabels({
        ...registration,
        labels: registration.labels,
      }),
      last_seen_ms: Date.now(),
      claim_state: previous?.claim_state ?? "available",
      claimed_by: previous?.claimed_by,
      lease_id: previous?.lease_id,
      lease_expires_at_ms: previous?.lease_expires_at_ms,
    };
    this.targets.set(record.target_id, record);
    this.writeState();
    this.writeContacts();
    return record;
  }

  listTargets(): TargetRecord[] {
    const now = Date.now();
    return [...this.targets.values()]
      .map((target) => ({
        ...target,
        labels: {
          ...target.labels,
          online: String(now - target.last_seen_ms < 20_000),
          lease_expired: String(Boolean(target.lease_expires_at_ms && target.lease_expires_at_ms <= now)),
          claim_state: target.claim_state,
        },
      }))
      .sort((left, right) => left.name.localeCompare(right.name));
  }

  labelsFor(targetId: string): Record<string, string> {
    const target = this.targets.get(targetId);
    if (!target) return {};
    const online = Date.now() - target.last_seen_ms < 20_000;
    return {
      ...target.labels,
      console_group: "Managed Targets",
      site: target.site,
      platform: target.platform,
      transport: target.transport,
      claim_state: target.claim_state,
      online: String(online),
      console_alert_level: online ? (target.claim_state === "available" ? "elevated" : "") : "critical",
    };
  }

  async claimTarget(targetId: string, operator = this.options.defaultOperator): Promise<TargetRecord> {
    const target = this.requireTarget(targetId);
    target.claim_state = "claimed";
    target.claimed_by = operator;
    target.lease_id = randomUUID();
    target.lease_expires_at_ms = Date.now() + 60_000;
    this.writeState();
    return target;
  }

  async releaseTarget(targetId: string): Promise<TargetRecord> {
    const target = this.requireTarget(targetId);
    target.claim_state = "available";
    delete target.claimed_by;
    delete target.lease_id;
    delete target.lease_expires_at_ms;
    this.writeState();
    return target;
  }

  async remoteTurn(targetId: string, request: RemoteTurnRequest): Promise<RemoteTurnResult> {
    const target = this.requireTarget(targetId);
    const controlUrl = `${target.control_addr}/turn`;
    const legacyUrl = `${target.legacy_addr}/turn`;
    const firstUrl = target.transport === "control" ? controlUrl : legacyUrl;
    const secondUrl = target.transport === "control" ? legacyUrl : controlUrl;
    try {
      return await postJson<RemoteTurnResult>(firstUrl, request, 20_000, this.options.targetAuthToken);
    } catch (firstError) {
      if (target.transport === "control") {
        try {
          const result = await postJson<RemoteTurnResult>(secondUrl, request, 20_000, this.options.targetAuthToken);
          return { ...result, transport: "legacy" };
        } catch {
          throw firstError;
        }
      }
      throw firstError;
    }
  }

  async respawnTarget(targetId: string): Promise<unknown> {
    const target = this.requireTarget(targetId);
    return postJson(`${target.control_addr}/respawn`, {}, 10_000, this.options.targetAuthToken);
  }

  async setModel(targetId: string, model: string): Promise<unknown> {
    const target = this.requireTarget(targetId);
    return postJson(`${target.control_addr}/model`, { model }, 10_000, this.options.targetAuthToken);
  }

  async refreshPeerPubkey(targetId: string): Promise<string> {
    const target = this.requireTarget(targetId);
    const result = await getJson<{ pubkey: string }>(`${target.control_addr}/peer_pubkey`, 10_000, this.options.targetAuthToken);
    target.pubkey = result.pubkey;
    this.writeState();
    this.writeContacts();
    return result.pubkey;
  }

  async fanout(prompt: string, operator = this.options.defaultOperator): Promise<RemoteTurnResult[]> {
    const targets = this.listTargets();
    const results: RemoteTurnResult[] = [];
    for (const target of targets) {
      results.push(await this.remoteTurn(target.target_id, { prompt, operator, handling_mode: "queue" }));
    }
    return results;
  }

  contactsToml(): string {
    const lines = ["[mobs]"];
    for (const target of this.listTargets()) {
      const endpoint = new URL(target.control_addr).host;
      const pubkey = target.pubkey.replace(/^ed25519:/, "");
      lines.push(
        `${JSON.stringify(target.target_id)} = { transport = "tcp://${endpoint}", pubkey = ${JSON.stringify(pubkey)} }`,
      );
    }
    return `${lines.join("\n")}\n`;
  }

  writeContacts(): void {
    writeFileSync(join(this.options.stateDir, "contacts.generated.toml"), this.contactsToml());
  }

  private statePath(): string {
    return join(this.options.stateDir, "kennel-state.json");
  }

  private loadState(): void {
    const path = this.statePath();
    if (!existsSync(path)) return;
    const state = JSON.parse(readFileSync(path, "utf8")) as KennelState;
    for (const target of state.targets ?? []) {
      this.targets.set(target.target_id, target);
    }
  }

  private writeState(): void {
    const state: KennelState = { targets: [...this.targets.values()] };
    writeFileSync(this.statePath(), JSON.stringify(state, null, 2));
  }

  private requireTarget(targetId: string): TargetRecord {
    const target = this.targets.get(targetId);
    if (!target) throw new Error(`unknown target: ${targetId}`);
    return target;
  }

  private async handleTargetAction(
    targetId: string,
    action: string,
    body: Record<string, unknown>,
  ): Promise<unknown> {
    switch (action) {
      case "claim":
        return { target: await this.claimTarget(targetId, String(body.operator ?? this.options.defaultOperator)) };
      case "release":
        return { target: await this.releaseTarget(targetId) };
      case "turn":
        return this.remoteTurn(targetId, {
          prompt: String(body.prompt ?? ""),
          operator: String(body.operator ?? this.options.defaultOperator),
          session_id: typeof body.session_id === "string" ? body.session_id : undefined,
          handling_mode: body.handling_mode === "steer" ? "steer" : "queue",
          model: typeof body.model === "string" ? body.model : undefined,
        });
      case "respawn":
        return this.respawnTarget(targetId);
      case "model":
        return this.setModel(targetId, String(body.model ?? ""));
      case "peer_pubkey":
        return { pubkey: await this.refreshPeerPubkey(targetId) };
      default:
        throw new Error(`unknown target action: ${action}`);
    }
  }
}
