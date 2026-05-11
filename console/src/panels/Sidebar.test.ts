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
