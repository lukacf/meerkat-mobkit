import assert from "node:assert/strict";
import test from "node:test";

import { normalizeAgents } from "./agents";

test("normalizeAgents preserves identity-status summary fields without invention", () => {
  const agents = normalizeAgents(
    {
      agent_sidebar: {
        live_snapshot: {
          agents: [
            {
              identity: " identity:luka ",
              display_name: " Luka ",
              role: " operator ",
              state: " running ",
              addressability: "addressable",
              generation: 4,
            checkpoint_version: 8,
            lease_healthy: true,
            labels: { team: " console " },
            model_capabilities: { image_input: true },
          },
        ],
      },
      },
    },
    [],
  );

  assert.equal(agents.length, 1);
  const agent = agents[0];
  assert.equal(agent?.identity, "identity:luka");
  assert.equal(agent?.member_id, "identity:luka");
  assert.equal(agent?.label, "Luka");
  assert.equal(agent?.role, "operator");
  assert.equal(agent?.state, "running");
  assert.equal(agent?.addressability, "addressable");
  assert.equal(agent?.generation, 4);
  assert.equal(agent?.checkpoint_version, 8);
  assert.equal(agent?.lease_healthy, true);
  assert.deepEqual(agent?.labels, { team: "console" });
});

test("normalizeAgents falls back to identity_status rows when sidebar snapshot is absent", () => {
  const agents = normalizeAgents(
    {
      identity_status: {
        schema_version: "1",
        refresh: { mode: "poll", interval_ms: 5000 },
        rows: [
          {
            identity: " identity:luka ",
            display_name: " Luka ",
            role: " operator ",
            state: " running ",
            addressability: "addressable",
            labels: { team: " console " },
            generation: 4,
            checkpoint_version: 8,
            lease_healthy: true,
            model_capabilities: { image_input: true },
          },
        ],
      },
    },
    [],
  );

  assert.equal(agents.length, 1);
  const agent = agents[0];
  assert.equal(agent?.identity, "identity:luka");
  assert.equal(agent?.member_id, "identity-only:identity:luka");
  assert.equal(agent?.label, "Luka");
  assert.equal(agent?.role, "operator");
  assert.equal(agent?.state, "running");
  assert.equal(agent?.addressability, "addressable");
  assert.equal(agent?.addressable, false);
  assert.equal(agent?.generation, 4);
  assert.equal(agent?.checkpoint_version, 8);
  assert.equal(agent?.lease_healthy, true);
  assert.deepEqual(agent?.labels, { team: "console" });
  assert.deepEqual(agent?.model_capabilities, { image_input: true });
  assert.deepEqual(agent?.affordances, { can_send_message: false });
});

test("normalizeAgents enriches sidebar snapshot rows with identity_status when both surfaces exist", () => {
  const agents = normalizeAgents(
    {
      agent_sidebar: {
        live_snapshot: {
          agents: [
            {
              agent_id: "domain:billing",
              member_id: "domain:billing",
              label: "Billing",
              kind: "module_agent",
              state: "running",
              addressable: true,
            },
          ],
        },
      },
      identity_status: {
        schema_version: "1",
        refresh: { mode: "poll", interval_ms: 5000 },
        rows: [
          {
            identity: "domain:billing",
            display_name: "Billing",
            role: "operator",
            state: "running",
            addressability: "addressable",
            labels: { team: "finance" },
            generation: 2,
            checkpoint_version: 3,
            lease_healthy: true,
            model_capabilities: { image_input: true },
          },
        ],
      },
    },
    [],
  );

  assert.equal(agents.length, 1);
  const agent = agents[0];
  assert.equal(agent?.identity, "domain:billing");
  assert.equal(agent?.member_id, "domain:billing");
  assert.equal(agent?.addressability, "addressable");
  assert.equal(agent?.addressable, true);
  assert.equal(agent?.generation, 2);
  assert.equal(agent?.checkpoint_version, 3);
  assert.equal(agent?.lease_healthy, true);
  assert.deepEqual(agent?.labels, { team: "finance" });
  assert.deepEqual(agent?.model_capabilities, { image_input: true });
});

test("normalizeAgents appends live identities missing from the sidebar snapshot", () => {
  const agents = normalizeAgents(
    {
      agent_sidebar: {
        live_snapshot: {
          agents: [
            {
              identity: "full-tools-worker-1",
              member_id: "full-tools-worker-1",
              label: "full-tools-worker-1",
              role: "full-tools-worker",
              state: "active",
              addressable: true,
            },
          ],
        },
      },
      identity_status: {
        schema_version: "1",
        rows: [
          {
            identity: "full-tools-worker-1",
            role: "full-tools-worker",
            state: "active",
            addressability: "addressable",
            labels: {},
          },
          {
            identity: "sub-worker-1",
            role: "identity",
            state: "active",
            addressability: "addressable",
            labels: {},
          },
        ],
      },
    },
    [],
  );

  assert.equal(agents.length, 2);
  const subWorker = agents.find((agent) => agent.identity === "sub-worker-1");
  assert.equal(subWorker?.member_id, "sub-worker-1");
  assert.equal(subWorker?.addressable, true);
  assert.deepEqual(subWorker?.affordances, { can_send_message: true });
});
