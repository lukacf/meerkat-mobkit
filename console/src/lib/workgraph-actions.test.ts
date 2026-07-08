import assert from "node:assert/strict";
import test from "node:test";

import { CONSOLE_COMMAND_NAMES, consoleCommandMethod } from "./headless";
import {
  resolveWorkGraphBindingRevision,
  resolveWorkGraphGoalItemRevision,
  resolveWorkGraphItemRevision,
  workGraphClaimOwnerId,
  type WorkGraphCommandRunner,
} from "./workgraph-actions";

function stubRunner(
  expectations: Array<{ command: string; params: Record<string, unknown>; result: unknown }>,
): { run: WorkGraphCommandRunner; callCount: () => number } {
  let index = 0;
  const run: WorkGraphCommandRunner = async (command, params) => {
    const expected = expectations[index];
    assert.ok(expected, `unexpected extra command ${command}`);
    index += 1;
    assert.equal(command, expected.command);
    assert.deepEqual(params, expected.params);
    return expected.result;
  };
  return { run, callCount: () => index };
}

test("claim owner id prefers the authenticated operator subject and falls back to the ops-lead id", () => {
  assert.equal(workGraphClaimOwnerId("luka@example.com", "console-ops-lead"), "luka@example.com");
  assert.equal(workGraphClaimOwnerId("  padded@example.com  ", "console-ops-lead"), "padded@example.com");
  assert.equal(workGraphClaimOwnerId("", "console-ops-lead"), "console-ops-lead");
  assert.equal(workGraphClaimOwnerId("   ", "console-ops-lead"), "console-ops-lead");
  assert.equal(workGraphClaimOwnerId(null, "console-ops-lead"), "console-ops-lead");
  assert.equal(workGraphClaimOwnerId(undefined, "console-ops-lead"), "console-ops-lead");
});

test("item revision resolution reads mobkit/workgraph/get and returns the live CAS token", async () => {
  const { run, callCount } = stubRunner([
    {
      command: CONSOLE_COMMAND_NAMES.workgraphGet,
      params: { id: "item-1" },
      result: { item: { id: "item-1", revision: 7, status: "open" } },
    },
  ]);
  assert.equal(await resolveWorkGraphItemRevision(run, "item-1"), 7);
  assert.equal(callCount(), 1);
});

test("item revision resolution fails loudly instead of guessing when the result carries no revision", async () => {
  const { run } = stubRunner([
    {
      command: CONSOLE_COMMAND_NAMES.workgraphGet,
      params: { id: "item-1" },
      result: { item: { id: "item-1" } },
    },
  ]);
  await assert.rejects(
    () => resolveWorkGraphItemRevision(run, "item-1"),
    /could not resolve the current revision of work item item-1/,
  );
});

test("item revision resolution propagates transport errors so nothing is sent", async () => {
  const run: WorkGraphCommandRunner = async () => {
    throw new Error("workgraph is not configured on this runtime");
  };
  await assert.rejects(
    () => resolveWorkGraphItemRevision(run, "item-1"),
    /not configured/,
  );
});

test("goal item revision resolution reads goal/status and CASes against the goal WORK ITEM", async () => {
  const { run } = stubRunner([
    {
      command: CONSOLE_COMMAND_NAMES.workgraphGoalStatus,
      params: { binding_id: "b-1" },
      result: {
        item: { id: "goal-1", revision: 4 },
        attention: { binding_id: "b-1", machine_state: { revision: 9 } },
      },
    },
  ]);
  assert.equal(await resolveWorkGraphGoalItemRevision(run, "b-1"), 4);
});

test("binding revision resolution reads goal/status and CASes against the binding machine state", async () => {
  const { run } = stubRunner([
    {
      command: CONSOLE_COMMAND_NAMES.workgraphGoalStatus,
      params: { binding_id: "b-1" },
      result: {
        item: { id: "goal-1", revision: 4 },
        attention: { binding_id: "b-1", machine_state: { revision: 9 } },
      },
    },
  ]);
  assert.equal(await resolveWorkGraphBindingRevision(run, "b-1"), 9);
});

test("binding revision resolution fails loudly when the machine state is absent", async () => {
  const { run } = stubRunner([
    {
      command: CONSOLE_COMMAND_NAMES.workgraphGoalStatus,
      params: { binding_id: "b-1" },
      result: { item: { id: "goal-1", revision: 4 }, attention: { binding_id: "b-1" } },
    },
  ]);
  await assert.rejects(
    () => resolveWorkGraphBindingRevision(run, "b-1"),
    /could not resolve the machine revision of attention binding b-1/,
  );
});

test("operator-result frames stamp the real RPC method behind each console command", () => {
  assert.equal(consoleCommandMethod(CONSOLE_COMMAND_NAMES.workgraphClaim), "mobkit/workgraph/claim");
  assert.equal(consoleCommandMethod(CONSOLE_COMMAND_NAMES.workgraphGet), "mobkit/workgraph/get");
  assert.equal(consoleCommandMethod(CONSOLE_COMMAND_NAMES.workgraphGoalStatus), "mobkit/workgraph/goal/status");
  assert.equal(
    consoleCommandMethod(CONSOLE_COMMAND_NAMES.workgraphGoalRequestClose),
    "mobkit/workgraph/goal/request_close",
  );
});
