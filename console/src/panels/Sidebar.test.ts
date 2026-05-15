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
