import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import YAML from "yaml";

import {
  MobKit,
  type MobHandle,
  type AgentBuildContext,
  type AgentBuildDraft,
  type AgentCustomizer,
  type DurableAgentSpec,
  type ManagedPeerEdge,
  type RosterProvider,
  type SessionAgentBuilder,
  SessionBuildOptions,
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
  kind?: string;
  name?: string;
  site?: string;
  platform?: string;
  address?: string;
  control_url?: string;
  peer_id?: string;
  public_key?: string;
  identity?: Record<string, unknown>;
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

function stringArg(args: Args, key: string): string | undefined {
  const value = args[key];
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function tomlString(value: string): string {
  return JSON.stringify(value);
}

function socketHost(socketAddress: string): string {
  if (socketAddress.startsWith("[")) {
    const end = socketAddress.indexOf("]");
    if (end > 0) return socketAddress.slice(1, end);
  }
  const colon = socketAddress.lastIndexOf(":");
  return colon >= 0 ? socketAddress.slice(0, colon) : socketAddress;
}

function isUnspecifiedHost(host: string): boolean {
  return host === "0.0.0.0" || host === "::" || host === "[::]";
}

function defaultSupervisorAdvertisedAddress(bindAddress: string): string {
  const host = socketHost(bindAddress);
  if (isUnspecifiedHost(host)) {
    throw new Error(
      "MDM supervisor bridge bind address is unspecified; set --supervisor-advertised tcp://<console-reachable-host>:<port> or MDM_SUPERVISOR_ADVERTISED_ADDRESS",
    );
  }
  return `tcp://${bindAddress}`;
}

function writeRuntimeMobConfig(args: Args, stateDir: string): {
  path: string;
  bindAddress: string;
  advertisedAddress: string;
} {
  const bindAddress =
    stringArg(args, "supervisor-bind") ??
    process.env.MDM_SUPERVISOR_BIND_ADDRESS ??
    "127.0.0.1:5790";
  const advertisedAddress =
    stringArg(args, "supervisor-advertised") ??
    process.env.MDM_SUPERVISOR_ADVERTISED_ADDRESS ??
    defaultSupervisorAdvertisedAddress(bindAddress);
  const source = readFileSync(join(configDir, "mob.toml"), "utf8");
  const path = join(stateDir, "mob.generated.toml");
  const content = `${source.trimEnd()}

[backend.external.supervisor_bridge]
bind_address = ${tomlString(bindAddress)}
advertised_address = ${tomlString(advertisedAddress)}
`;
  writeFileSync(path, content);
  return { path, bindAddress, advertisedAddress };
}

function agentCommsAddress(args: Args): string {
  return (
    stringArg(args, "agent-comms-address") ??
    process.env.MDM_AGENT_COMMS_ADDRESS ??
    "127.0.0.1:5793"
  );
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
  const entries = Array.isArray(parsed) ? parsed : [parsed];
  return entries.map((target, index) => {
    if (!target || typeof target !== "object") {
      throw new Error(`remote target ${index} must be an object`);
    }
    const value = target as RemoteTargetBinding;
    if (!value.id) throw new Error(`remote target ${index} is missing id`);
    return value;
  });
}

function bindingFor(target: RemoteTargetBinding): Record<string, unknown> {
  const source = target.binding ?? target;
  const address = typeof source.address === "string" ? source.address : target.address;
  const bootstrapToken =
    typeof source.bootstrap_token === "string"
      ? source.bootstrap_token
      : target.bootstrap_token;
  const identity =
    source.identity && typeof source.identity === "object"
      ? source.identity
      : target.identity && typeof target.identity === "object"
        ? target.identity
        : undefined;
  const publicKey =
    typeof source.public_key === "string"
      ? source.public_key
      : typeof target.public_key === "string"
        ? target.public_key
        : undefined;

  if (!address || (!identity && !publicKey)) {
    throw new Error(
      `remote target ${target.id} must provide either binding or address + public_key`,
    );
  }

  return {
    kind: "external",
    address,
    bootstrap_token: bootstrapToken,
    identity: identity ?? {
      kind: "ed25519_public_key",
      public_key: publicKey,
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
      const spec: DurableAgentSpec = {
        identity: target.id,
        profile: "target",
        addressability: "addressable",
        displayName,
        labels: {
          ...targetLabels(target),
          display_name: displayName,
          transport: "mob_remote",
        },
        context: { target },
        additionalInstructions: [
          "You are a visible MDM target agent backed by a runtime on the target host.",
          "For every host/system/hardware/process/file/network question, inspect the real host before answering.",
          "Do not answer target-machine questions from roster labels or binding metadata.",
        ],
        runtimeModeOverride: "turn_driven",
      };
      spec.backend = "external";
      spec.binding = bindingFor(target);
      return spec;
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
          "When the operator asks targets a question that requires an answer, call send_request on each target peer and wait for the correlated PeerResponse.",
          "Do not use send_message for target questions that need a report; send_message is one-way and will not provide a response to summarize.",
          "Do not answer target machine questions from labels, metadata, or assumptions.",
          "If no target peers are reachable, say that explicitly instead of inventing a target answer.",
        ],
      },
      ...targetSpecs,
    ];
  }
}

class MdmSessionBuilder implements SessionAgentBuilder {
  constructor(private readonly targets: RemoteTargetBinding[]) {}

  async buildAgent(options: SessionBuildOptions): Promise<void> {
    const identity = options.labels.durable_identity;
    if (identity && identity !== "hive") {
      const target = this.targets.find((candidate) => candidate.id === identity);
      if (!target) return;
      options.additionalInstructions.push(
        "You are a remote managed target. Inspect this host with your target-side shell tools for host/system/hardware questions.",
        "Never answer target-machine questions from labels or binding metadata.",
      );
      return;
    }
    if (identity !== "hive") return;
    options.additionalInstructions.push(
      "Use the mob peer roster/comms tools to discover and message target peers.",
      "For operator questions about target hosts, use send_request with a clear inspection intent for each target peer, wait for the correlated PeerResponse, and summarize the response.",
      "Do not use send_message for host inspection questions because it is one-way and gives you nothing to wait for.",
      "Target answers must come back through the mob peer path, not through target metadata or side-channel control endpoints.",
    );
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

type ConsoleAccepted = {
  interaction_id: string;
  session_id?: string;
};

type ConsoleFrame = {
  cursor?: string;
  identity?: string;
  kind?: string;
  interaction_id?: string;
  session_id?: string;
  payload?: Record<string, unknown>;
};

async function sendConsoleTargetTurn(
  baseUrl: string,
  identity: string,
  prompt: string,
): Promise<ConsoleAccepted> {
  const response = await fetch(`${baseUrl}/console/send`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      identity,
      content: prompt,
      origin: "mdm-real-target-smoke",
      idempotency_key: `mdm-real-target-smoke-${identity}-${Date.now()}`,
      handling_mode: "queue",
    }),
  });
  const text = await response.text();
  if (!response.ok) {
    throw new Error(`console send to ${identity} returned ${response.status}: ${text}`);
  }
  const accepted = JSON.parse(text) as ConsoleAccepted;
  if (!accepted.interaction_id) {
    throw new Error(`console send to ${identity} returned no interaction_id: ${text}`);
  }
  return accepted;
}

async function waitForDeliveryCompletion(
  baseUrl: string,
  identity: string,
  interactionId: string,
): Promise<ConsoleFrame> {
  const deadline = Date.now() + 120_000;
  let lastPayload = "";
  while (Date.now() < deadline) {
    const url = new URL(`${baseUrl}/console/timeline`);
    url.searchParams.set("identity", identity);
    url.searchParams.set("limit", "200");
    const response = await fetch(url);
    const text = await response.text();
    if (!response.ok) {
      throw new Error(`/console/timeline returned ${response.status}: ${text}`);
    }
    const page = JSON.parse(text) as { frames?: ConsoleFrame[] };
    for (const frame of page.frames ?? []) {
      if (
        frame.identity === identity &&
        frame.interaction_id === interactionId &&
        frame.payload?.source_event_type === "delivery_completion" &&
        typeof frame.payload.text === "string" &&
        frame.payload.text.trim()
      ) {
        return frame;
      }
    }
    lastPayload = text;
    await new Promise((resolve) => setTimeout(resolve, 1_000));
  }
  throw new Error(
    `timed out waiting for delivery_completion from ${identity} interaction=${interactionId}; last timeline=${lastPayload.slice(0, 1000)}`,
  );
}

async function waitForIdentityTextContaining(
  baseUrl: string,
  identity: string,
  required: string[],
): Promise<ConsoleFrame> {
  const deadline = Date.now() + 180_000;
  let lastPayload = "";
  while (Date.now() < deadline) {
    const url = new URL(`${baseUrl}/console/timeline`);
    url.searchParams.set("identity", identity);
    url.searchParams.set("limit", "300");
    const response = await fetch(url);
    const text = await response.text();
    if (!response.ok) {
      throw new Error(`/console/timeline returned ${response.status}: ${text}`);
    }
    const page = JSON.parse(text) as { frames?: ConsoleFrame[] };
    for (const frame of page.frames ?? []) {
      const content = typeof frame.payload?.text === "string" ? frame.payload.text : "";
      const lower = content.toLowerCase();
      if (
        frame.identity === identity &&
        frame.kind === "interaction_complete" &&
        content.trim() &&
        required.every((needle) => lower.includes(needle.toLowerCase()))
      ) {
        return frame;
      }
    }
    lastPayload = text;
    await new Promise((resolve) => setTimeout(resolve, 1_000));
  }
  throw new Error(
    `timed out waiting for ${identity} timeline text containing ${required.join(", ")}; last timeline=${lastPayload.slice(0, 1000)}`,
  );
}

async function wireHiveToTargets(
  handle: MobHandle,
  targets: RemoteTargetBinding[],
): Promise<void> {
  const members = await handle.listMembers();
  const activeMembers = new Set(members.map((member) => member.agentIdentity));
  const hive = members.find((member) => member.agentIdentity.startsWith("rt:hive:"));
  if (!hive) {
    throw new Error(
      `expected hive runtime member in mob roster, got: ${[...activeMembers].join(", ")}`,
    );
  }

  for (const target of targets) {
    if (!activeMembers.has(target.id)) {
      throw new Error(
        `expected remote target mob member '${target.id}', got: ${[...activeMembers].join(", ")}`,
      );
    }
    await handle.wireMember(hive.agentIdentity, target.id);
    console.log(`[mdm] wired hive ${hive.agentIdentity} <-> ${target.id}`);
  }
}

async function runRealTargetSmoke(
  baseUrl: string,
  handle: MobHandle,
  targets: RemoteTargetBinding[],
): Promise<void> {
  if (targets.length === 0) {
    throw new Error("real target smoke requires at least one target binding");
  }
  const members = await handle.listMembers();
  const activeMembers = new Set(members.map((member) => member.agentIdentity));
  for (const target of targets) {
    if (!activeMembers.has(target.id)) {
      throw new Error(
        `real target smoke expected active mob member '${target.id}', got: ${[...activeMembers].join(", ")}`,
      );
    }
  }

  for (const target of targets) {
    const prompt = [
      "MDM real-target smoke.",
      "This is a peer turn delivered through the MobKit/Meerkat mob path.",
      "Inspect the local target host before answering.",
      "Report hostname, OS/kernel, current user, and whether shell tools are available.",
      "Do not answer from roster labels or binding metadata.",
    ].join(" ");
    const accepted = await sendConsoleTargetTurn(baseUrl, target.id, prompt);
    const completion = await waitForDeliveryCompletion(
      baseUrl,
      target.id,
      accepted.interaction_id,
    );
    console.log(
      `[mdm-real-target-smoke] ${target.id}: ok session=${completion.session_id ?? accepted.session_id ?? "unknown"}`,
    );
  }
}

async function runHiveTargetSmoke(
  baseUrl: string,
  handle: MobHandle,
  targets: RemoteTargetBinding[],
): Promise<void> {
  if (targets.length === 0) {
    throw new Error("hive target smoke requires at least one target binding");
  }
  const members = await handle.listMembers();
  const activeMembers = new Set(members.map((member) => member.agentIdentity));
  const hive = members.find((member) => member.agentIdentity.startsWith("rt:hive:"));
  if (!hive) {
    throw new Error(
      `hive target smoke expected autonomous hive member, got: ${[...activeMembers].join(", ")}`,
    );
  }
  for (const target of targets) {
    if (!activeMembers.has(target.id)) {
      throw new Error(
        `hive target smoke expected active mob member '${target.id}', got: ${[...activeMembers].join(", ")}`,
      );
    }
  }

  const token = `mdm-hive-smoke-${Date.now()}`;
  const prompt = [
    `Autonomous hive smoke checksum_token=${token}.`,
    "Use the mob peer roster/comms tools to find every target peer.",
    "For each target peer, call send_request with the checksum token and ask the target to inspect its own host.",
    "Wait for correlated PeerResponse results before answering.",
    "Summarize hostname, OS/kernel, current user, CPU, memory, and shell tools per target.",
    "Do not answer from roster labels, binding metadata, or console-side state.",
    "Include the checksum token in your final answer.",
  ].join(" ");
  await sendConsoleTargetTurn(baseUrl, "hive", prompt);
  const completion = await waitForIdentityTextContaining(baseUrl, "hive", [
    token,
    "hostname",
    "cpu",
    "memory",
  ]);
  const text =
    typeof completion.payload?.text === "string" ? completion.payload.text : "";
  console.log(
    `[mdm-hive-target-smoke] ok hive=${hive.agentIdentity} token=${token} cursor=${completion.cursor ?? "unknown"} answer=${text.slice(0, 160).replace(/\s+/g, " ")}`,
  );
}

async function main() {
  const args = parseArgs();
  const skipBuild = Boolean(args["skip-build"]);
  const smoke = Boolean(args.smoke);
  const realTargetSmoke = Boolean(args["real-target-smoke"]);
  const hiveTargetSmoke = Boolean(args["hive-target-smoke"]);
  const wait = Boolean(args.wait) || Boolean(args["browser-smoke"]);
  const hiveKickoff = stringArg(args, "hive-kickoff");
  const useDemoLlm = Boolean(args["demo-llm"]) || !process.env.OPENAI_API_KEY;
  const targets = readTargetBindings(args);
  if (targets.length === 0 && !Boolean(args["allow-empty-targets"])) {
    throw new Error(
      "no remote targets configured; pass --targets <json> or set MDM_REMOTE_TARGETS_JSON",
    );
  }

  const stateDir = stringArg(args, "state-dir") ?? join(here, ".state");
  mkdirSync(stateDir, { recursive: true });
  const mobConfig = writeRuntimeMobConfig(args, stateDir);
  const localAgentCommsAddress = agentCommsAddress(args);
  let builder = MobKit.builder()
    .mob(mobConfig.path)
    .gateway(ensureGatewayBin(skipBuild))
    .agentComms({ mode: "tcp", address: localAgentCommsAddress })
    .consoleConfig(join(configDir, "console.toml"))
    .consoleAuthRequired(false)
    .consoleFetchTimeoutMs(120_000)
    .persistentState(stateDir)
    .sessionService(new MdmSessionBuilder(targets))
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
      supervisor_bridge: mobConfig.advertisedAddress,
    });
    await handle.reconcileEdges();
    await wireHiveToTargets(handle, targets);
    const baseUrl = runtime.rustHttpBaseUrl;
    if (!baseUrl) throw new Error("MobKit runtime did not expose an HTTP console URL");
    console.log(`[mdm] console: ${baseUrl}/console`);
    console.log(`[mdm] remote-targets: ${targets.length}`);
    console.log(`[mdm] supervisor-bridge: ${mobConfig.advertisedAddress}`);
    console.log(`[mdm] local-agent-comms: tcp://${localAgentCommsAddress}`);

    if (smoke || realTargetSmoke || hiveTargetSmoke) {
      const title = await getConsoleTitle(baseUrl);
      if (title !== scenario.console_expected_title) {
        throw new Error(`unexpected console title: ${String(title)}`);
      }
    }
    if (realTargetSmoke) {
      await runRealTargetSmoke(baseUrl, handle, targets);
      console.log("[mdm-real-target-smoke] ok");
    }
    if (hiveTargetSmoke) {
      if (useDemoLlm) {
        throw new Error(
          "hive target smoke requires a real model; set OPENAI_API_KEY and do not pass --demo-llm",
        );
      }
      await runHiveTargetSmoke(baseUrl, handle, targets);
    }
    if (smoke) {
      console.log("[mdm-smoke] ok");
      return;
    }
    if (hiveKickoff) {
      console.log("[mdm] sending hive kickoff");
      await runtime.send("hive", hiveKickoff);
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
