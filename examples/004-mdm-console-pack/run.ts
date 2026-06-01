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
  type TopologyProvider,
} from "../../sdk/typescript/src/index.ts";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "../..");
const configDir = join(here, "config");
const scenarioPath = join(here, "scenario.yaml");

type ScenarioTarget = {
  id: string;
  name: string;
  site: string;
  platform: string;
  labels?: Record<string, string>;
};

type Scenario = {
  scenario_id: string;
  default_operator: string;
  console_expected_title: string;
  targets: ScenarioTarget[];
  links: Array<[string, string]>;
};

type RemoteTargetBinding = {
  id: string;
  name?: string;
  site?: string;
  platform?: string;
  address?: string;
  public_key?: string;
  bootstrap_token?: string;
  binding?: Record<string, unknown>;
  labels?: Record<string, string>;
};

type Args = Record<string, string | boolean>;

const scenario = YAML.parse(readFileSync(scenarioPath, "utf8")) as Scenario;

function parseArgs(argv = process.argv.slice(2)): Args {
  const result: Args = {};
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (!arg.startsWith("--")) continue;
    const key = arg.slice(2);
    const next = argv[index + 1];
    if (next && !next.startsWith("--")) {
      result[key] = next;
      index += 1;
    } else {
      result[key] = true;
    }
  }
  return result;
}

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

function readTargetBindings(args: Args): RemoteTargetBinding[] {
  const explicitPath =
    typeof args.targets === "string"
      ? args.targets
      : process.env.MDM_REMOTE_TARGETS_FILE;
  const raw =
    process.env.MDM_REMOTE_TARGETS_JSON ??
    (explicitPath ? readFileSync(resolve(explicitPath), "utf8") : "[]");
  const parsed = JSON.parse(raw) as unknown;
  if (!Array.isArray(parsed)) {
    throw new Error("remote targets must be a JSON array");
  }
  return parsed.map((target, index) => {
    if (!target || typeof target !== "object") {
      throw new Error(`remote target ${index} must be an object`);
    }
    const value = target as RemoteTargetBinding;
    if (!value.id) throw new Error(`remote target ${index} is missing id`);
    return value;
  });
}

function bindingFor(target: RemoteTargetBinding): Record<string, unknown> {
  if (target.binding) return target.binding;
  if (!target.address || !target.public_key) {
    throw new Error(
      `remote target ${target.id} must provide either binding or address + public_key`,
    );
  }
  return {
    kind: "external",
    address: target.address,
    bootstrap_token: target.bootstrap_token,
    identity: {
      kind: "ed25519_public_key",
      public_key: target.public_key,
    },
  };
}

function scenarioTarget(id: string): ScenarioTarget | undefined {
  return scenario.targets.find((target) => target.id === id);
}

function targetLabels(target: RemoteTargetBinding): Record<string, string> {
  const scenarioEntry = scenarioTarget(target.id);
  return {
    ...(scenarioEntry?.labels ?? {}),
    ...(target.labels ?? {}),
    console_group: "Managed Targets",
    durable_identity: target.id,
    site: target.site ?? scenarioEntry?.site ?? "remote",
    platform: target.platform ?? scenarioEntry?.platform ?? "unknown",
    transport: "mob_remote",
    claim_state: "remote",
  };
}

class MdmRosterProvider implements RosterProvider {
  constructor(private readonly targets: RemoteTargetBinding[]) {}

  async roster(): Promise<DurableAgentSpec[]> {
    const targetSpecs: DurableAgentSpec[] = this.targets.map((target) => {
      const scenarioEntry = scenarioTarget(target.id);
      const displayName = target.name ?? scenarioEntry?.name ?? target.id;
      return {
        identity: target.id,
        profile: "target",
        addressability: "addressable",
        displayName,
        labels: {
          ...targetLabels(target),
          display_name: displayName,
        },
        context: null,
        additionalInstructions: [],
        runtimeModeOverride: "turn_driven",
        backend: "external",
        binding: bindingFor(target),
      };
    });
    return [
      {
        identity: "hive",
        profile: "hive",
        addressability: "addressable",
        displayName: "Hive",
        labels: {
          console_group: "Fleet Control",
          site: "all",
          platform: "mobkit",
          claim_state: "coordinator",
          durable_identity: "hive",
        },
        context: null,
        additionalInstructions: [
          "Targets are real mob peers, not records in a target registry.",
          "When the operator asks targets a question, send the question to the target peers through comms and wait for their replies.",
          "Do not answer target machine questions from labels, metadata, or assumptions.",
        ],
      },
      ...targetSpecs,
    ];
  }
}

class MdmTopologyProvider implements TopologyProvider {
  constructor(private readonly targets: RemoteTargetBinding[]) {}

  async computeEdges(targetIdentities: string[]): Promise<ManagedPeerEdge[]> {
    const identities = new Set(targetIdentities);
    const edges: ManagedPeerEdge[] = [];
    const add = (a: string, b: string) => {
      if (identities.has(a) && identities.has(b) && a !== b) edges.push({ a, b });
    };
    for (const [a, b] of scenario.links) add(a, b);
    for (const target of this.targets) add("hive", target.id);
    return edges;
  }
}

class MdmCustomizer implements AgentCustomizer {
  async customizeBuild(
    _context: AgentBuildContext,
    spec: DurableAgentSpec,
    draft: AgentBuildDraft,
  ): Promise<void> {
    draft.labels = {
      ...draft.labels,
      ...spec.labels,
      sdk_toolbelt: "mdm-mob-roster",
    };
    draft.additionalInstructions.push(...spec.additionalInstructions);
  }
}

async function getConsoleTitle(baseUrl: string): Promise<string | undefined> {
  const response = await fetch(`${baseUrl}/console/experience`);
  if (!response.ok) throw new Error(`/console/experience returned ${response.status}`);
  const experience = (await response.json()) as {
    console_config?: { title?: string };
  };
  return experience.console_config?.title;
}

async function main() {
  const args = parseArgs();
  const skipBuild = Boolean(args["skip-build"]);
  const smoke = Boolean(args.smoke);
  const wait = Boolean(args.wait) || Boolean(args["browser-smoke"]);
  const useDemoLlm = Boolean(args["demo-llm"]) || !process.env.OPENAI_API_KEY;
  const targets = readTargetBindings(args);
  if (targets.length === 0 && !Boolean(args["allow-empty-targets"])) {
    throw new Error(
      "no remote targets configured; pass --targets <json> or set MDM_REMOTE_TARGETS_JSON",
    );
  }

  const stateDir = join(here, ".state");
  mkdirSync(stateDir, { recursive: true });

  let builder = MobKit.builder()
    .mob(join(configDir, "mob.toml"))
    .gateway(ensureGatewayBin(skipBuild))
    .consoleConfig(join(configDir, "console.toml"))
    .consoleAuthRequired(false)
    .consoleFetchTimeoutMs(120_000)
    .persistentState(stateDir)
    .rosterProvider(new MdmRosterProvider(targets))
    .topologyProvider(new MdmTopologyProvider(targets))
    .agentCustomizer(new MdmCustomizer());
  if (useDemoLlm) builder = builder.demoLlm();
  const runtime = await builder.build();

  try {
    const handle = runtime.mobHandle();
    await handle.setMobLabels({
      example_pack: "004-mdm-console",
      scenario: scenario.scenario_id,
      remote_targets: String(targets.length),
      remote_topology: "mob-roster",
    });
    await handle.reconcileEdges();
    const baseUrl = runtime.rustHttpBaseUrl;
    if (!baseUrl) throw new Error("MobKit runtime did not expose an HTTP console URL");
    console.log(`[mdm] console: ${baseUrl}/console`);
    console.log(`[mdm] remote-targets: ${targets.length}`);

    if (smoke) {
      const title = await getConsoleTitle(baseUrl);
      if (title !== scenario.console_expected_title) {
        throw new Error(`unexpected console title: ${String(title)}`);
      }
      console.log("[mdm-smoke] ok");
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
  }
}

main().catch((error) => {
  console.error(error instanceof Error ? error.stack : error);
  process.exitCode = 1;
});
