import { describe, it } from "node:test";
import assert from "node:assert/strict";

import {
  MobHandle,
  binaryQuestion,
  chooseOneQuestion,
  gradeQuestion,
  parseDecisionResult,
} from "../src/index.js";

function createMockRuntime() {
  const calls: Array<{ method: string; params?: unknown }> = [];
  let responder: (method: string, params?: unknown) => unknown = () => ({});
  const runtime = {
    async _rpc(method: string, params?: unknown) {
      calls.push({ method, params });
      return responder(method, params);
    },
  };
  const handle = new MobHandle(runtime as never);
  return {
    handle,
    calls,
    setResponse(next: (method: string, params?: unknown) => unknown) {
      responder = next;
    },
  };
}

const WIRE_RESULT = {
  contract: "v1",
  route: { backend: "llm", provider: "openai", model: "gpt-5.5" },
  judgments: {
    is_urgent: { judgment: { kind: "binary", form: "categorical", answer: "yes" } },
    department: {
      judgment: { kind: "choice", form: "selected", option: "billing" },
      native_signals: [
        {
          signal: "choice_distribution",
          backend: "jev",
          probabilities: { billing: 0.88, technical: 0.12 },
          confidence: 0.81,
        },
      ],
    },
    frustration: { judgment: { kind: "grade", form: "level", index: 1 } },
    mood: { judgment: { kind: "grade", form: "native_weighted", position: 1.05 } },
    fit: { judgment: { kind: "choice", form: "abstain" } },
  },
  accounting: { kind: "unmeasured" },
  budget: { kind: "not_issued" },
  attempts: 2,
};

describe("decision question builders", () => {
  it("emit the typed wire shape", () => {
    assert.deepEqual(binaryQuestion("is_urgent", "Does this convey urgency?"), {
      kind: "binary",
      id: "is_urgent",
      instructions: "Does this convey urgency?",
    });
    assert.deepEqual(
      chooseOneQuestion("department", "Which team?", { billing: "Payments", technical: "Bugs" }),
      {
        kind: "choose_one",
        id: "department",
        instructions: "Which team?",
        options: [
          { id: "billing", description: "Payments" },
          { id: "technical", description: "Bugs" },
        ],
      },
    );
    assert.deepEqual(gradeQuestion("frustration", "How frustrated?", ["Calm", "Angry"]), {
      kind: "grade",
      id: "frustration",
      instructions: "How frustrated?",
      levels: [{ description: "Calm" }, { description: "Angry" }],
    });
  });
});

describe("MobHandle.decide()", () => {
  it("sends mobkit/decision/evaluate and parses the typed result", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => WIRE_RESULT);

    const result = await handle.decide(
      "Help! My payouts have been failing for 3 days.",
      [binaryQuestion("is_urgent", "Does this convey urgency?")],
      { task: "Triage" },
    );

    assert.equal(calls[0].method, "mobkit/decision/evaluate");
    assert.deepEqual(calls[0].params, {
      state: "Help! My payouts have been failing for 3 days.",
      questions: [{ kind: "binary", id: "is_urgent", instructions: "Does this convey urgency?" }],
      task: "Triage",
    });
    assert.equal(result.contract, "v1");
    assert.equal(result.route.backend, "llm");
    assert.equal(result.attempts, 2);
    assert.deepEqual(result.accounting, { kind: "unmeasured" });
    assert.deepEqual(result.budget, { kind: "not_issued" });
    assert.equal(result.judgments.is_urgent.answer, "yes");
    assert.equal(result.judgments.department.option, "billing");
    assert.equal(result.judgments.department.nativeSignals.length, 1);
    assert.equal(result.judgments.frustration.levelIndex, 1);
    assert.equal(result.judgments.mood.weightedPosition, 1.05);
    assert.equal(result.judgments.mood.levelIndex, null, "no level is elected by the SDK");
    assert.equal(result.judgments.fit.isAbstain, true);
  });

  it("omits task when not given", async () => {
    const { handle, calls, setResponse } = createMockRuntime();
    setResponse(() => WIRE_RESULT);
    await handle.decide({ records: [] }, [binaryQuestion("q", "x")]);
    assert.deepEqual(calls[0].params, {
      state: { records: [] },
      questions: [{ kind: "binary", id: "q", instructions: "x" }],
    });
  });

  it("parseDecisionResult reports absent fields as null instead of inventing values", () => {
    const parsed = parseDecisionResult({ contract: "v1" });
    assert.deepEqual(parsed.judgments, {});
    assert.equal(parsed.attempts, null);
    assert.equal(parseDecisionResult({}).contract, null);
  });
});
