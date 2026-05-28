import assert from "node:assert/strict";
import test from "node:test";

import type { ConsoleAgent } from "../types";
import { __sidebarTest } from "./Sidebar";

test("sidebar treats broad-profile worker wired to commander as a child, not the host", () => {
  const commander: ConsoleAgent = {
    agent_id: "incident-commander",
    member_id: "incident-commander",
    identity: "incident-commander",
    label: "Incident Commander",
    kind: "mob_agent",
    role: "commander",
    group: "Coordinators",
    wired_to: ["full-tools-worker-2"],
  };
  const worker: ConsoleAgent = {
    agent_id: "full-tools-worker-2",
    member_id: "full-tools-worker-2",
    identity: "full-tools-worker-2",
    label: "full-tools-worker-2",
    kind: "mob_agent",
    role: "commander",
    group: "commander",
    wired_to: ["incident-commander"],
  };

  assert.equal(__sidebarTest.isCommanderLike(worker), false);
  assert.equal(__sidebarTest.isCommanderLike(commander), true);
  assert.equal(__sidebarTest.isSpawnedDelegateLike(worker, commander), true);
});

test("sidebar resolves worker-spawned workers under their worker host", () => {
  const commander: ConsoleAgent = {
    agent_id: "incident-commander",
    member_id: "incident-commander",
    identity: "incident-commander",
    label: "Incident Commander",
    kind: "mob_agent",
    role: "commander",
    group: "Coordinators",
    wired_to: ["tutti-profile-worker"],
  };
  const worker: ConsoleAgent = {
    agent_id: "tutti-profile-worker",
    member_id: "tutti-profile-worker",
    identity: "tutti-profile-worker",
    label: "tutti-profile-worker",
    kind: "mob_agent",
    role: "worker",
    group: "worker",
    wired_to: ["incident-commander", "standby-worker"],
  };
  const subWorker: ConsoleAgent = {
    agent_id: "standby-worker",
    member_id: "standby-worker",
    identity: "standby-worker",
    label: "standby-worker",
    kind: "mob_agent",
    role: "worker",
    group: "worker",
    wired_to: ["tutti-profile-worker"],
  };
  const agents = [commander, worker, subWorker];

  assert.equal(__sidebarTest.findSpawnHost(worker, agents, commander)?.member_id, "incident-commander");
  assert.equal(__sidebarTest.findSpawnHost(subWorker, agents, commander)?.member_id, "tutti-profile-worker");
  assert.deepEqual(
    __sidebarTest.groupSidebarAgents(agents).get("Coordinators")?.map((row) => [
      row.agent.member_id,
      row.depth,
    ]),
    [
      ["incident-commander", 0],
      ["tutti-profile-worker", 1],
      ["standby-worker", 2],
    ],
  );
});

test("sidebar nests workers under wired non-worker hosts", () => {
  const investigator: ConsoleAgent = {
    agent_id: "rt:deep-investigator:singleton:0",
    member_id: "rt:deep-investigator:singleton:0",
    identity: "deep-investigator:singleton",
    label: "Deep Investigator",
    kind: "mob_agent",
    role: "deep-investigator",
    group: "Coordinators",
    wired_to: ["investigation-worker-daily-candy"],
  };
  const worker: ConsoleAgent = {
    agent_id: "investigation-worker-daily-candy",
    member_id: "investigation-worker-daily-candy",
    identity: "investigation-worker-daily-candy",
    label: "investigation-worker-daily-candy",
    kind: "mob_agent",
    role: "investigation-worker",
    group: "worker",
    wired_to: ["deep-investigator:singleton"],
  };
  const agents = [worker, investigator];

  assert.equal(__sidebarTest.findSpawnHost(worker, agents, null)?.member_id, "rt:deep-investigator:singleton:0");
  assert.deepEqual(
    __sidebarTest.groupSidebarAgents(agents).get("Coordinators")?.map((row) => [
      row.agent.member_id,
      row.depth,
    ]),
    [
      ["rt:deep-investigator:singleton:0", 0],
      ["investigation-worker-daily-candy", 1],
    ],
  );
});

test("sidebar configured grouping keeps spawned investigation workers under their coordinator host", () => {
  const investigator: ConsoleAgent = {
    agent_id: "rt:deep-investigator:singleton:0",
    member_id: "rt:deep-investigator:singleton:0",
    identity: "deep-investigator:singleton",
    label: "Deep Investigator",
    kind: "mob_agent",
    role: "deep-investigator",
    group: "Coordinators",
    wired_to: ["investigation-worker-nested-spawn-1"],
  };
  const worker: ConsoleAgent = {
    agent_id: "investigation-worker-nested-spawn-1",
    member_id: "investigation-worker-nested-spawn-1",
    identity: "investigation-worker-nested-spawn-1",
    label: "investigation-worker-nested-spawn-1",
    kind: "mob_agent",
    role: "investigation-worker",
    group: "investigation-worker",
    wired_to: ["deep-investigator:singleton"],
  };
  const grouped = __sidebarTest.groupSidebarAgents([worker, investigator], {
    group_by: ["labels.console_group", "group", "role"],
    section_order: ["Coordinators", "investigation-worker"],
  });

  assert.equal(grouped.has("investigation-worker"), false);
  assert.deepEqual(
    grouped.get("Coordinators")?.map((row) => [row.agent.member_id, row.depth]),
    [
      ["rt:deep-investigator:singleton:0", 0],
      ["investigation-worker-nested-spawn-1", 1],
    ],
  );
});

test("sidebar resolves worker-spawned workers from implicit delegate parent refs", () => {
  const commander: ConsoleAgent = {
    agent_id: "incident-commander",
    member_id: "incident-commander",
    identity: "incident-commander",
    label: "Incident Commander",
    kind: "mob_agent",
    role: "commander",
    group: "Coordinators",
    wired_to: ["incident-worker-full-1"],
  };
  const worker: ConsoleAgent = {
    agent_id: "incident-worker-full-1",
    member_id: "incident-worker-full-1",
    identity: "incident-worker-full-1",
    label: "incident-worker-full-1",
    kind: "mob_agent",
    role: "worker",
    group: "worker",
    wired_to: ["incident-commander", "implicit-019e186d-48e4-7da1-9559-4d17155ab30d/delegate/cardinalpay-support-worker-1"],
  };
  const subWorker: ConsoleAgent = {
    agent_id: "cardinalpay-support-worker-1",
    member_id: "cardinalpay-support-worker-1",
    identity: "cardinalpay-support-worker-1",
    label: "cardinalpay-support-worker-1",
    kind: "mob_agent",
    role: "worker",
    group: "worker",
    wired_to: ["implicit-019e186d-48e4-7da1-9559-4d17155ab30d/delegate/incident-worker-full-1"],
  };
  const agents = [commander, subWorker, worker];

  assert.equal(__sidebarTest.findSpawnHost(worker, agents, commander)?.member_id, "incident-commander");
  assert.equal(__sidebarTest.findSpawnHost(subWorker, agents, commander)?.member_id, "incident-worker-full-1");
  assert.deepEqual(
    __sidebarTest.groupSidebarAgents(agents).get("Coordinators")?.map((row) => [
      row.agent.member_id,
      row.depth,
    ]),
    [
      ["incident-commander", 0],
      ["incident-worker-full-1", 1],
      ["cardinalpay-support-worker-1", 2],
    ],
  );
});

test("sidebar nests delegate-created workers even when runtime groups them as coordinators", () => {
  const commander: ConsoleAgent = {
    agent_id: "incident-commander",
    member_id: "incident-commander",
    identity: "incident-commander",
    label: "Incident Commander",
    kind: "mob_agent",
    role: "commander",
    group: "Coordinators",
    wired_to: ["qa2-parent-worker"],
  };
  const parentWorker: ConsoleAgent = {
    agent_id: "qa2-parent-worker",
    member_id: "qa2-parent-worker",
    identity: "qa2-parent-worker",
    label: "qa2-parent-worker",
    kind: "mob_agent",
    role: "delegate",
    group: "Coordinators",
    labels: {
      delegate_host_identity: "incident-commander",
      source_mob_id: "implicit-019e22a9-4e67-7f62-a9e2-3d96c8d43439",
    },
    wired_to: [
      "incident-command-center/commander/incident-commander",
      "incident-commander",
    ],
  };
  const childWorker: ConsoleAgent = {
    agent_id: "qa2-child-worker",
    member_id: "qa2-child-worker",
    identity: "qa2-child-worker",
    label: "qa2-child-worker",
    kind: "mob_agent",
    role: "delegate",
    group: "Coordinators",
    labels: {
      delegate_host_identity: "qa2-parent-worker",
      source_mob_id: "implicit-019e22ab-64c8-7d43-a19a-2e12cab16f0f",
    },
    wired_to: [
      "implicit-019e22a9-4e67-7f62-a9e2-3d96c8d43439/delegate/qa2-parent-worker",
      "qa2-parent-worker",
    ],
  };
  const agents = [commander, childWorker, parentWorker];

  assert.equal(__sidebarTest.findSpawnHost(parentWorker, agents, commander)?.member_id, "incident-commander");
  assert.equal(__sidebarTest.findSpawnHost(childWorker, agents, commander)?.member_id, "qa2-parent-worker");
  assert.deepEqual(
    __sidebarTest.groupSidebarAgents(agents).get("Coordinators")?.map((row) => [
      row.agent.member_id,
      row.depth,
    ]),
    [
      ["incident-commander", 0],
      ["qa2-parent-worker", 1],
      ["qa2-child-worker", 2],
    ],
  );
});

test("sidebar resolves encoded identity references while nesting workers", () => {
  const commander: ConsoleAgent = {
    agent_id: "identity:luka",
    member_id: "identity:luka",
    identity: "identity:luka",
    label: "Incident Commander",
    kind: "mob_agent",
    role: "commander",
    group: "Coordinators",
    wired_to: ["identity/worker"],
  };
  const worker: ConsoleAgent = {
    agent_id: "identity:worker",
    member_id: "identity:worker",
    identity: "identity:worker",
    label: "identity-worker",
    kind: "mob_agent",
    role: "worker",
    group: "worker",
    wired_to: ["identity/luka"],
  };

  assert.equal(__sidebarTest.findSpawnHost(worker, [commander, worker], commander)?.member_id, "identity:luka");
});

test("sidebar can group agents by configured metadata selectors and subgroups", () => {
  const agents: ConsoleAgent[] = [
    {
      agent_id: "initiative:billing",
      member_id: "initiative:billing",
      identity: "initiative:billing",
      label: "Billing Initiative",
      kind: "mob_agent",
      role: "initiative",
      group: "Domains",
      labels: {
        console_group: "Initiatives",
        org: "Payments",
      },
    },
    {
      agent_id: "initiative:comms",
      member_id: "initiative:comms",
      identity: "initiative:comms",
      label: "Comms Initiative",
      kind: "mob_agent",
      role: "initiative",
      group: "Domains",
      labels: {
        console_group: "Initiatives",
        org: "Customer",
      },
    },
    {
      agent_id: "identity:luka",
      member_id: "identity:luka",
      identity: "identity:luka",
      label: "Luka",
      kind: "mob_agent",
      role: "identity",
      group: "Personal",
      labels: {},
    },
  ];

  const grouped = __sidebarTest.groupSidebarAgents(agents, {
    group_by: ["labels.console_group", "group"],
    subgroup_by: ["labels.org"],
    section_order: ["Personal", "Initiatives"],
    badges: [
      { id: "org", label: "Org", field: "labels.org" },
    ],
  });

  assert.deepEqual(
    grouped.get("Initiatives")?.map((row) => [row.agent.member_id, row.subgroup]),
    [
      ["initiative:comms", "Customer"],
      ["initiative:billing", "Payments"],
    ],
  );
  assert.deepEqual(
    grouped.get("Personal")?.map((row) => row.agent.member_id),
    ["identity:luka"],
  );
  assert.deepEqual(
    __sidebarTest.configuredAgentBadges(agents[0], {
      badges: [
        { id: "org", label: "Org", field: "labels.org" },
      ],
    }),
    [{ id: "org", label: "Org", value: "Payments", tone: undefined }],
  );
});

class MemoryStorage {
  private readonly values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.has(key) ? this.values.get(key)! : null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

const ob3Grouping = {
  group_by: ["labels.group", "role"],
  subgroup_by: ["labels.scope_id"],
  section_order: ["coordinators", "initiatives", "workers", "recipients"],
  collapse_single_subgroup: true,
};

function ob3Agent(args: {
  id: string;
  label: string;
  group: string;
  scope: string;
  role?: string;
  identity?: string;
  labelIdentity?: string;
}): ConsoleAgent {
  return {
    agent_id: `rt:${args.id}`,
    member_id: `member:${args.id}`,
    identity: args.identity,
    label: args.label,
    kind: "mob_agent",
    role: args.role || args.group,
    labels: {
      group: args.group,
      scope_id: args.scope,
      scope_ids: args.scope,
      ...(args.labelIdentity ? { agent_identity: args.labelIdentity } : {}),
    },
  };
}

function virtualRowsForAgents(
  agents: ConsoleAgent[],
  options: {
    collapsedSections?: Set<string>;
    collapsedSubgroups?: Set<string>;
    pinnedAgentIds?: Set<string>;
    searchActive?: boolean;
  } = {},
) {
  const grouped = __sidebarTest.groupSidebarAgents(agents, ob3Grouping);
  const sectionNames = __sidebarTest.orderedSectionNames(grouped, ob3Grouping);
  return __sidebarTest.buildSidebarVirtualRows({
    sectionNames,
    grouped,
    grouping: ob3Grouping,
    collapsedSections: options.collapsedSections || new Set(),
    collapsedSubgroups: options.collapsedSubgroups || new Set(),
    pinnedAgentIds: options.pinnedAgentIds,
    searchActive: options.searchActive,
  });
}

test("sidebar section collapse storage overrides config defaults after first load", () => {
  const storage = new MemoryStorage();
  const storageKey = __sidebarTest.sidebarStorageKey("sections", "runtime-a");
  const grouping = {
    sections: [
      { name: "initiatives", collapsed: true },
      { name: "workers", collapsed: false },
    ],
  };

  assert.deepEqual(
    Array.from(__sidebarTest.collapsedSectionsForStorage(grouping, storageKey, storage)),
    ["initiatives"],
  );

  __sidebarTest.writeSidebarStringSet(storage, storageKey, new Set(["workers"]));

  assert.deepEqual(
    Array.from(__sidebarTest.collapsedSectionsForStorage(grouping, storageKey, storage)),
    ["workers"],
  );
});

test("sidebar subgroup headers render and can remove their agents when collapsed", () => {
  const agents = [
    ob3Agent({ id: "initiative-cto", label: "CTO Initiative", group: "initiatives", scope: "cto", role: "initiative" }),
    ob3Agent({ id: "initiative-liveops", label: "LiveOps Initiative", group: "initiatives", scope: "liveops", role: "initiative" }),
  ];
  const expanded = virtualRowsForAgents(agents);

  assert.deepEqual(
    expanded.filter((row) => row.kind === "subgroup").map((row) => [row.bucket, row.label, row.collapsed]),
    [
      ["initiatives", "cto", false],
      ["initiatives", "liveops", false],
    ],
  );

  const collapsed = virtualRowsForAgents(agents, {
    collapsedSubgroups: new Set([__sidebarTest.sidebarSubgroupStorageId("initiatives", "cto")]),
  });

  assert.deepEqual(
    collapsed.filter((row) => row.kind === "agent").map((row) => row.row.agent.member_id),
    ["member:initiative-liveops"],
  );
});

test("sidebar subgroup collapse state persists through storage", () => {
  const storage = new MemoryStorage();
  const storageKey = __sidebarTest.sidebarStorageKey("subgroups", "runtime-a");
  const subgroupKey = __sidebarTest.sidebarSubgroupStorageId("initiatives", "liveops");
  __sidebarTest.writeSidebarStringSet(storage, storageKey, new Set([subgroupKey]));

  assert.deepEqual(
    Array.from(__sidebarTest.collapsedSubgroupsForStorage(storageKey, storage)),
    [subgroupKey],
  );
});

test("sidebar search expands section and subgroup matches without mutating saved collapse state", () => {
  const agents = [
    ob3Agent({ id: "initiative-cto", label: "CTO Initiative", group: "initiatives", scope: "cto", role: "initiative" }),
    ob3Agent({ id: "initiative-liveops", label: "LiveOps Initiative", group: "initiatives", scope: "liveops", role: "initiative" }),
  ];
  const collapsedSections = new Set(["initiatives"]);
  const collapsedSubgroups = new Set([__sidebarTest.sidebarSubgroupStorageId("initiatives", "cto")]);
  const rows = virtualRowsForAgents(agents, {
    collapsedSections,
    collapsedSubgroups,
    searchActive: true,
  });

  assert.deepEqual(
    rows.filter((row) => row.kind === "agent").map((row) => row.row.agent.member_id),
    ["member:initiative-cto", "member:initiative-liveops"],
  );
  assert.equal(collapsedSections.has("initiatives"), true);
  assert.equal(collapsedSubgroups.has(__sidebarTest.sidebarSubgroupStorageId("initiatives", "cto")), true);
});

test("sidebar pinned agents sort first inside their configured subgroup only", () => {
  const agents = [
    ob3Agent({ id: "initiative-alpha", label: "Alpha", group: "initiatives", scope: "cto", role: "initiative", identity: "initiative:alpha" }),
    ob3Agent({ id: "initiative-beta", label: "Beta", group: "initiatives", scope: "cto", role: "initiative", identity: "initiative:beta" }),
    ob3Agent({ id: "initiative-gamma", label: "Gamma", group: "initiatives", scope: "liveops", role: "initiative", identity: "initiative:gamma" }),
  ];
  const rows = virtualRowsForAgents(agents, {
    pinnedAgentIds: new Set(["initiative:beta", "initiative:gamma"]),
  });

  assert.deepEqual(
    rows.filter((row) => row.kind === "agent").map((row) => [row.bucket, row.row.subgroup, row.row.agent.identity]),
    [
      ["initiatives", "cto", "initiative:beta"],
      ["initiatives", "cto", "initiative:alpha"],
      ["initiatives", "liveops", "initiative:gamma"],
    ],
  );
});

test("sidebar pin ids prefer durable identity and labels.agent_identity before member_id", () => {
  assert.equal(
    __sidebarTest.sidebarAgentPinId(ob3Agent({
      id: "durable",
      label: "Durable",
      group: "workers",
      scope: "cto",
      identity: "agent:durable",
    })),
    "agent:durable",
  );
  assert.equal(
    __sidebarTest.sidebarAgentPinId(ob3Agent({
      id: "label-identity",
      label: "Label Identity",
      group: "workers",
      scope: "cto",
      labelIdentity: "agent:from-label",
    })),
    "agent:from-label",
  );
  assert.equal(
    __sidebarTest.sidebarAgentPinId(ob3Agent({
      id: "member-only",
      label: "Member Only",
      group: "workers",
      scope: "cto",
    })),
    "member:member-only",
  );
});

test("sidebar configured OB3-like grouping yields configured sections and scope subgroups", () => {
  const agents = [
    ob3Agent({ id: "coord", label: "Coordinator", group: "coordinators", scope: "cto", role: "coordinator" }),
    ob3Agent({ id: "initiative-cto", label: "CTO Initiative", group: "initiatives", scope: "cto", role: "initiative" }),
    ob3Agent({ id: "initiative-liveops", label: "LiveOps Initiative", group: "initiatives", scope: "liveops", role: "initiative" }),
    ob3Agent({ id: "initiative-game-production", label: "Game Production", group: "initiatives", scope: "game-production", role: "initiative" }),
    ob3Agent({ id: "initiative-game-platform", label: "Game Platform", group: "initiatives", scope: "game-platform", role: "initiative" }),
    ob3Agent({ id: "worker", label: "Worker", group: "workers", scope: "game-platform", role: "worker" }),
    ob3Agent({ id: "recipient", label: "Recipient", group: "recipients", scope: "liveops", role: "recipient" }),
  ];
  const grouped = __sidebarTest.groupSidebarAgents(agents, ob3Grouping);
  const sections = __sidebarTest.orderedSectionNames(grouped, ob3Grouping);
  const rows = __sidebarTest.buildSidebarVirtualRows({
    sectionNames: sections,
    grouped,
    grouping: { ...ob3Grouping, collapse_single_subgroup: false },
    collapsedSections: new Set(),
    collapsedSubgroups: new Set(),
  });

  assert.deepEqual(sections, ["coordinators", "initiatives", "workers", "recipients"]);
  assert.deepEqual(
    rows
      .filter((row) => row.kind === "subgroup" && row.bucket === "initiatives")
      .map((row) => row.label),
    ["cto", "game-platform", "game-production", "liveops"],
  );
  assert.deepEqual(
    grouped.get("initiatives")?.map((row) => [row.agent.member_id, row.subgroup]),
    [
      ["member:initiative-cto", "cto"],
      ["member:initiative-game-platform", "game-platform"],
      ["member:initiative-game-production", "game-production"],
      ["member:initiative-liveops", "liveops"],
    ],
  );
});
