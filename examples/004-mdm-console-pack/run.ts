import { existsSync, mkdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import YAML from "yaml";

import {
  MobKit,
  type AgentBuildContext,
  type AgentBuildDraft,
  type AgentCustomizer,
  type DurableAgentSpec,
  type ManagedPeerEdge,
  type RosterProvider,
  type SessionAgentBuilder,
  type SessionBuildOptions,
  type TopologyProvider,
} from "../../sdk/typescript/src/index.ts";

import { MdmKennel } from "./src/kennel.js";
import { startTargetDaemon } from "./src/targetd.js";
import {
  authHeaders,
  getJson,
  parseArgs,
  postJson,
  targetLabels,
  waitFor,
  type ProcessHandle,
  type Scenario,
  type ScenarioTarget,
  type TargetRecord,
} from "./src/protocol.js";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../..");
const configDir = join(here, "config");
const scenarioPath = join(here, "scenario.yaml");
const scenario = YAML.parse(readFileSync(scenarioPath, "utf8")) as Scenario;

type ToolContext = {
  kind: "hive" | "target";
  targetId?: string;
  kennelUrl: string;
  kennelAuthToken?: string;
  operator: string;
};

function repoCargoEnv(): Record<string, string> {
  const result = spawnSync(join(repoRoot, "scripts/repo-cargo"), ["--print-env"], {
    cwd: repoRoot,
    encoding: "utf8",
  });
  if (result.status !== 0) {
    throw new Error(`repo-cargo --print-env failed:\n${result.stderr}`);
  }
  return Object.fromEntries(
    result.stdout
      .trim()
      .split(/\n/)
      .map((line) => line.split("=", 2))
      .filter(([key, value]) => key && value),
  );
}

function ensureGatewayBin(skipBuild: boolean): string {
  if (process.env.MOBKIT_RPC_GATEWAY_BIN) return process.env.MOBKIT_RPC_GATEWAY_BIN;
  const env = repoCargoEnv();
  const gateway = join(env.CARGO_TARGET_DIR, "debug", "rpc_gateway");
  if (!skipBuild || !existsSync(gateway)) {
    const result = spawnSync(
      join(repoRoot, "scripts/repo-cargo"),
      ["build", "-p", "meerkat-mobkit", "--bin", "rpc_gateway"],
      { cwd: repoRoot, stdio: "inherit" },
    );
    if (result.status !== 0) throw new Error("failed to build rpc_gateway");
  }
  return gateway;
}

function targetFromScenario(targetId: string): ScenarioTarget | undefined {
  return scenario.targets.find((target) => target.id === targetId);
}

function labelsForTarget(target: ScenarioTarget, registered?: TargetRecord): Record<string, string> {
  return {
    ...targetLabels(target),
    ...(registered?.labels ?? {}),
    console_group: "Managed Targets",
    display_name: target.name,
    durable_identity: target.id,
    claim_state: registered?.claim_state ?? "available",
    online: String(Boolean(registered)),
    transport: registered?.transport ?? target.transport,
    console_alert_level: registered ? (registered.claim_state === "available" ? "elevated" : "") : "critical",
  };
}

class MdmRosterProvider implements RosterProvider {
  constructor(
    private readonly kennelUrl: string,
    private readonly operator: string,
    private readonly kennelAuthToken?: string,
  ) {}

  async roster(): Promise<DurableAgentSpec[]> {
    const registered = await getJson<{ targets: TargetRecord[] }>(
      `${this.kennelUrl}/api/targets`,
      10_000,
      this.kennelAuthToken,
    )
      .catch(() => ({ targets: [] }));
    const byId = new Map(registered.targets.map((target) => [target.target_id, target]));
    const targets: DurableAgentSpec[] = scenario.targets.map((target) => ({
      identity: target.id,
      profile: "target_proxy",
      addressability: "addressable",
      displayName: target.name,
      labels: labelsForTarget(target, byId.get(target.id)),
      context: {
        kind: "target",
        targetId: target.id,
        kennelUrl: this.kennelUrl,
        kennelAuthToken: this.kennelAuthToken,
        operator: this.operator,
      } satisfies ToolContext,
      additionalInstructions: [
        `You are the console proxy for remote target ${target.name}.`,
        `The target lives at site=${target.site}, platform=${target.platform}.`,
      ],
    }));
    return [
      {
        identity: "hive",
        profile: "hive",
        addressability: "addressable",
        displayName: "Hive",
        labels: {
          console_group: "Fleet Control",
          site: "all",
          platform: "kennel",
          claim_state: "coordinator",
          durable_identity: "hive",
        },
        context: {
          kind: "hive",
          kennelUrl: this.kennelUrl,
          kennelAuthToken: this.kennelAuthToken,
          operator: this.operator,
        } satisfies ToolContext,
        additionalInstructions: [
          "You are the fleet-level MDM hive. Use MDM tools before making fleet claims.",
        ],
      },
      ...targets,
    ];
  }
}

class MdmTopologyProvider implements TopologyProvider {
  async computeEdges(targetIdentities: string[]): Promise<ManagedPeerEdge[]> {
    const targets = new Set(targetIdentities);
    const edges: ManagedPeerEdge[] = [];
    const add = (a: string, b: string) => {
      if (targets.has(a) && targets.has(b) && a !== b) edges.push({ a, b });
    };
    for (const [a, b] of scenario.links) add(a, b);
    return edges;
  }
}

class MdmCustomizer implements AgentCustomizer {
  constructor(
    private readonly kennelUrl: string,
    private readonly kennelAuthToken?: string,
  ) {}

  async customizeBuild(
    _context: AgentBuildContext,
    spec: DurableAgentSpec,
    draft: AgentBuildDraft,
  ): Promise<void> {
    if (spec.identity === "hive") return;
    const target = targetFromScenario(spec.identity);
    if (!target) return;
    const registered = await getJson<{ target: TargetRecord }>(
      `${this.kennelUrl}/api/targets/${target.id}`,
      10_000,
      this.kennelAuthToken,
    )
      .then((value) => value.target)
      .catch(() => undefined);
    draft.labels = {
      ...draft.labels,
      ...labelsForTarget(target, registered),
    };
  }
}

function mdmTools(context: ToolContext): Record<string, (args: Record<string, unknown>) => Promise<unknown>> {
  const targetId = () => {
    const value = context.targetId;
    if (!value) throw new Error("tool requires target context");
    return value;
  };
  return {
    async mdm_target_status(args) {
      const id = String(args.target_id ?? targetId());
      return getJson(`${context.kennelUrl}/api/targets/${id}`, 10_000, context.kennelAuthToken);
    },
    async mdm_remote_turn(args) {
      const id = String(args.target_id ?? targetId());
      return postJson(
        `${context.kennelUrl}/api/targets/${id}/turn`,
        {
          prompt: String(args.prompt ?? args.command ?? ""),
          operator: String(args.operator ?? context.operator),
          handling_mode: args.handling_mode === "steer" ? "steer" : "queue",
          model: typeof args.model === "string" ? args.model : undefined,
        },
        10_000,
        context.kennelAuthToken,
      );
    },
    async mdm_claim_target(args) {
      const id = String(args.target_id ?? targetId());
      return postJson(
        `${context.kennelUrl}/api/targets/${id}/claim`,
        {
          operator: String(args.operator ?? context.operator),
        },
        10_000,
        context.kennelAuthToken,
      );
    },
    async mdm_release_target(args) {
      const id = String(args.target_id ?? targetId());
      return postJson(`${context.kennelUrl}/api/targets/${id}/release`, {}, 10_000, context.kennelAuthToken);
    },
    async mdm_respawn_target(args) {
      const id = String(args.target_id ?? targetId());
      return postJson(`${context.kennelUrl}/api/targets/${id}/respawn`, {}, 10_000, context.kennelAuthToken);
    },
    async mdm_set_model(args) {
      const id = String(args.target_id ?? targetId());
      return postJson(
        `${context.kennelUrl}/api/targets/${id}/model`,
        {
          model: String(args.model ?? "demo-target-model"),
        },
        10_000,
        context.kennelAuthToken,
      );
    },
    async mdm_list_targets() {
      return getJson(`${context.kennelUrl}/api/targets`, 10_000, context.kennelAuthToken);
    },
    async mdm_hive_fanout(args) {
      const prompt = String(args.prompt ?? "");
      const targets = await getJson<{ targets: TargetRecord[] }>(
        `${context.kennelUrl}/api/targets`,
        10_000,
        context.kennelAuthToken,
      );
      const results = [];
      for (const target of targets.targets) {
        results.push(
          await postJson(
            `${context.kennelUrl}/api/targets/${target.target_id}/turn`,
            {
              prompt,
              operator: context.operator,
              handling_mode: "queue",
            },
            10_000,
            context.kennelAuthToken,
          ),
        );
      }
      return { prompt, results };
    },
  };
}

class MdmSessionBuilder implements SessionAgentBuilder {
  async buildAgent(options: SessionBuildOptions): Promise<void> {
    const context = (options.appContext ?? {}) as ToolContext;
    options.additionalInstructions.push(
      "MDM tools are authoritative for target state and remote execution.",
      "When using mdm_remote_turn, quote the returned target text rather than inventing output.",
    );
    options.labels = {
      ...options.labels,
      sdk_toolbelt: "mdm-console",
    };
    for (const [name, handler] of Object.entries(mdmTools(context))) {
      options.registerTool(name, handler);
    }
  }
}

async function spawnScenarioTargets(
  kennelUrl: string,
  kennelAuthToken?: string,
  targetAuthToken?: string,
): Promise<ProcessHandle[]> {
  const handles: ProcessHandle[] = [];
  const baseState = join(here, ".target-state");
  mkdirSync(baseState, { recursive: true });
  for (const target of scenario.targets) {
    handles.push(await startTargetDaemon({
      id: target.id,
      name: target.name,
      site: target.site,
      platform: target.platform,
      transport: target.transport,
      listen: `127.0.0.1:${target.port}`,
      kennelUrl,
      kennelAuthToken,
      controlAuthToken: targetAuthToken,
      stateDir: join(baseState, target.id),
      allowShell: true,
      labels: target.labels ?? {},
    }));
  }
  return handles;
}

async function runSmoke(kennelUrl: string, consoleUrl: string, kennelAuthToken?: string): Promise<void> {
  await waitFor("registered targets", async () => {
    const response = await getJson<{ targets: TargetRecord[] }>(
      `${kennelUrl}/api/targets`,
      10_000,
      kennelAuthToken,
    );
    return response.targets.length >= scenario.targets.length;
  });
  const target = scenario.targets[1] ?? scenario.targets[0];
  await postJson(
    `${kennelUrl}/api/targets/${target.id}/claim`,
    { operator: scenario.default_operator },
    10_000,
    kennelAuthToken,
  );
  const turn = await postJson<{ text: string }>(
    `${kennelUrl}/api/targets/${target.id}/turn`,
    {
      prompt: "shell: echo MOBKIT_MDM_SMOKE",
      operator: scenario.default_operator,
    },
    10_000,
    kennelAuthToken,
  );
  if (!turn.text.includes("MOBKIT_MDM_SMOKE")) {
    throw new Error(`remote smoke turn did not include marker: ${turn.text}`);
  }
  const experience = await getJson<Record<string, unknown>>(`${consoleUrl}/console/experience`);
  const consoleConfig = experience.console_config as Record<string, unknown> | undefined;
  if (consoleConfig?.title !== scenario.console_expected_title) {
    throw new Error(`unexpected console title: ${String(consoleConfig?.title)}`);
  }
  const contacts = await fetch(`${kennelUrl}/api/contacts.toml`, {
    headers: authHeaders(kennelAuthToken),
  }).then((response) => response.text());
  if (!contacts.includes(target.id)) throw new Error("generated contacts did not include target");
  console.log("[mdm-smoke] ok");
}

async function main() {
  const args = parseArgs();
  const skipBuild = Boolean(args["skip-build"]);
  const spawnTargets = Boolean(args["spawn-targets"]) || Boolean(args.smoke) || Boolean(args["browser-smoke"]);
  const smoke = Boolean(args.smoke);
  const wait = Boolean(args.wait) || Boolean(args["browser-smoke"]);
  const apiOnly = Boolean(args["api-only"]);
  const useDemoLlm = Boolean(args["demo-llm"]) || !process.env.OPENAI_API_KEY;
  const operator = String(args.operator ?? scenario.default_operator);
  const kennelAuthToken = typeof args["auth-token"] === "string" ? args["auth-token"] : process.env.MDM_AUTH_TOKEN;
  const targetAuthToken =
    typeof args["target-auth-token"] === "string" ? args["target-auth-token"] : process.env.MDM_TARGET_AUTH_TOKEN;
  const requireAuth = Boolean(args["require-auth"]) || process.env.MDM_REQUIRE_AUTH === "true";
  const requireTls = Boolean(args["require-tls"]) || process.env.MDM_REQUIRE_TLS === "true";
  const tlsCertPath = typeof args["tls-cert"] === "string" ? args["tls-cert"] : process.env.MDM_TLS_CERT_PATH;
  const tlsKeyPath = typeof args["tls-key"] === "string" ? args["tls-key"] : process.env.MDM_TLS_KEY_PATH;
  if (requireAuth && (!kennelAuthToken || !targetAuthToken)) {
    throw new Error("--require-auth requires MDM_AUTH_TOKEN and MDM_TARGET_AUTH_TOKEN");
  }
  if (requireTls && (!tlsCertPath || !tlsKeyPath)) {
    throw new Error("--require-tls requires MDM_TLS_CERT_PATH and MDM_TLS_KEY_PATH");
  }
  const expectedTargets = Number(
    args["expect-targets"] ?? process.env.MDM_EXPECT_TARGETS ?? (spawnTargets ? scenario.targets.length : 0),
  );
  const apiListen = String(args["api-listen"] ?? process.env.MDM_API_LISTEN_ADDR ?? scenario.api_listen_addr);
  const stateDir = join(here, ".state");
  mkdirSync(stateDir, { recursive: true });

  const kennel = new MdmKennel({
    listen: apiListen,
    stateDir,
    defaultOperator: operator,
    authToken: kennelAuthToken,
    targetAuthToken,
    tlsCertPath,
    tlsKeyPath,
  });
  const kennelHandle = await kennel.start();
  const targetHandles = spawnTargets
    ? await spawnScenarioTargets(kennelHandle.url, kennelAuthToken, targetAuthToken)
    : [];
  await waitFor("target registration", async () => {
    if (expectedTargets <= 0) return true;
    return kennel.listTargets().filter((target) => target.labels.online === "true").length >= expectedTargets;
  });

  if (apiOnly) {
    console.log(`[mdm] api: ${kennelHandle.url}`);
    console.log(`[mdm] contacts: ${join(stateDir, "contacts.generated.toml")}`);
    try {
      if (wait) {
        await new Promise<void>((resolve) => {
          process.once("SIGINT", resolve);
          process.once("SIGTERM", resolve);
        });
      }
    } finally {
      await Promise.all(targetHandles.map((handle) => handle.close()));
      await kennelHandle.close();
    }
    return;
  }

  let builder = MobKit.builder()
    .mob(join(configDir, "mob.toml"))
    .gateway(ensureGatewayBin(skipBuild))
    .consoleConfig(join(configDir, "console.toml"))
    .consoleAuthRequired(false)
    .consoleFetchTimeoutMs(120_000)
    .persistentState(stateDir)
    .sessionService(new MdmSessionBuilder())
    .rosterProvider(new MdmRosterProvider(kennelHandle.url, operator, kennelAuthToken))
    .topologyProvider(new MdmTopologyProvider())
    .agentCustomizer(new MdmCustomizer(kennelHandle.url, kennelAuthToken));
  if (useDemoLlm) builder = builder.demoLlm();
  const runtime = await builder.build();

  try {
    const handle = runtime.mobHandle();
    await handle.setMobLabels({
      example_pack: "004-mdm-console",
      scenario: scenario.scenario_id,
      remote_targets: String(kennel.targets.size),
    });
    const baseUrl = runtime.rustHttpBaseUrl;
    if (!baseUrl) throw new Error("MobKit runtime did not expose an HTTP console URL");
    console.log(`[mdm] api: ${kennelHandle.url}`);
    console.log(`[mdm] console: ${baseUrl}/console`);
    console.log(`[mdm] contacts: ${join(stateDir, "contacts.generated.toml")}`);

    if (smoke) {
      await runSmoke(kennelHandle.url, baseUrl, kennelAuthToken);
      return;
    }
    if (wait) {
      await new Promise<void>((resolve) => {
        process.once("SIGINT", resolve);
        process.once("SIGTERM", resolve);
      });
    }
  } finally {
    await runtime.shutdown();
    await Promise.all(targetHandles.map((handle) => handle.close()));
    await kennelHandle.close();
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack : error);
  process.exitCode = 1;
});
