import { describe, expect, it } from "vitest";
import { beginConsoleSendAttempt, createConsoleSendAttempt, finishConsoleSendAttempt, recoverConsoleSendAttempt, consoleSendFailureState } from "./send-attempt";

const draft = () => createConsoleSendAttempt({ id: "a", scope: "authority/realm/principal", destination: "agent", origin: "console:pane-a", idempotencyKey: "key", text: "  hi\r\n🌳  ", now: 1 });
describe("durable send attempts", () => {
  it("freezes exact content, original pane, scope and key before dispatch", () => {
    const attempted = beginConsoleSendAttempt(draft(), { owner: "tab1", now: 2, handlingMode: "queue" });
    expect(JSON.parse(attempted.envelopeJson!)).toEqual({ identity: "agent", content: "  hi\r\n🌳  ", origin: "console:pane-a", origin_kind: "operator", idempotency_key: "key", handling_mode: "queue" });
    const rejected = finishConsoleSendAttempt(attempted, { state: "definitely-rejected", error: "invalid" });
    expect(beginConsoleSendAttempt(rejected, { owner: "tab2", now: 3, handlingMode: "queue", retryRejected: true }).envelopeJson).toBe(attempted.envelopeJson);
  });
  it("does not replay a possible accepted write after lease expiry or reload", () => {
    const attempted = beginConsoleSendAttempt(draft(), { owner: "tab1", now: 2, handlingMode: "queue" });
    const reloaded = recoverConsoleSendAttempt(JSON.parse(JSON.stringify(attempted)), 20_000);
    expect(reloaded.state).toBe("outcome-unknown");
    expect(reloaded.envelopeJson).toBe(attempted.envelopeJson);
    expect(() => beginConsoleSendAttempt(reloaded, { owner: "tab2", now: 21_000, handlingMode: "queue" })).toThrow(/Reconcile/);
    expect(() => beginConsoleSendAttempt(reloaded, { owner: "tab2", now: 21_000, handlingMode: "steer" })).toThrow(/Reconcile/);
  });
  it("prevents promotion of an already attempted queue item", () => {
    const attempt = beginConsoleSendAttempt(draft(), { owner: "tab1", now: 2, handlingMode: "queue" });
    const rejected = finishConsoleSendAttempt(attempt, { state: "definitely-rejected", error: "denied" });
    expect(() => beginConsoleSendAttempt(rejected, { owner: "tab2", now: 3, handlingMode: "steer", retryRejected: true })).toThrow(/handling mode/);
  });
  it("retains canonical acceptance and rejects empty acceptance", () => {
    const attempt = beginConsoleSendAttempt(draft(), { owner: "t", now: 2, handlingMode: "steer" });
    expect(finishConsoleSendAttempt(attempt, { state: "accepted", interactionId: "i", inputFrameId: "f" }).accepted).toEqual({ interactionId: "i", inputFrameId: "f" });
    expect(() => finishConsoleSendAttempt(attempt, { state: "accepted", interactionId: "" })).toThrow(/prove acceptance/);
  });
  it("treats network loss, in-flight and idempotency conflicts conservatively", () => {
    for (const error of [new Error("lost response"), { rpcError: { code: -32009, data: { kind: "idempotency_conflict" } } }, { rpcError: { code: -32001 } }]) {
      expect(consoleSendFailureState(error)).toBe("outcome-unknown");
    }
    expect(consoleSendFailureState({ rpcError: { code: -32602 } })).toBe("definitely-rejected");
  });
});

import { reconcileConsoleSendReceipt } from "./send-attempt";
it("reconciles only owner receipts matching exact envelope and destination", () => {
  const attempted = beginConsoleSendAttempt(draft(), { owner: "tab1", now: 2, handlingMode: "queue" });
  const unknown = finishConsoleSendAttempt(attempted, { state: "outcome-unknown", error: "timeout" });
  const frame = { id: "frame", event: "user_input", identity: "agent", interactionId: "interaction", data: JSON.parse(attempted.envelopeJson!) };
  expect(reconcileConsoleSendReceipt(unknown, frame)?.accepted?.inputFrameId).toBe("frame");
  expect(reconcileConsoleSendReceipt(unknown, { ...frame, identity: "other" })).toBeNull();
  expect(reconcileConsoleSendReceipt(unknown, { ...frame, data: { ...frame.data, origin: "console:different-pane" } })).toBeNull();
  expect(reconcileConsoleSendReceipt(unknown, { ...frame, data: { ...frame.data, content: "edited" } })).toBeNull();
});

it("reconciles text-block receipts across JSON key ordering without changing any text bytes", () => {
  const context = { version: 1 as const, id: "q", sourceScope: "source", sourceIdentity: "reader", messageId: "m", quote: "  A\u030a\r\n🚀  ", label: "Source" };
  const attempted = beginConsoleSendAttempt({ ...draft(), contexts: [context] }, { owner: "tab", now: 2, handlingMode: "queue" });
  const envelope = JSON.parse(attempted.envelopeJson!);
  const content = envelope.content.map((block: { type: string; text: string }) => ({ text: block.text, type: block.type }));
  const frame = { id: "frame", event: "user_input", identity: "agent", interactionId: "interaction", data: { ...envelope, content } };
  expect(reconcileConsoleSendReceipt(attempted, frame)?.accepted?.inputFrameId).toBe("frame");
  expect(reconcileConsoleSendReceipt(attempted, { ...frame, data: { ...frame.data, content: [...content].reverse() } })).toBeNull();
  expect(reconcileConsoleSendReceipt(attempted, { ...frame, data: { ...frame.data, content: content.map((block: { type: string; text: string }) => ({ ...block, text: block.text.trim() })) } })).toBeNull();
});

import { validateConsoleSendAttempt } from "./send-attempt";
it("rejects malformed identifiers, leases and acceptance receipts", () => {
  const attempted = beginConsoleSendAttempt(draft(), { owner: "tab", now: 2, handlingMode: "queue" });
  const accepted = finishConsoleSendAttempt(attempted, { state: "accepted", interactionId: "owner-receipt" });
  for (const malformed of [
    { ...draft(), id: 123 }, { ...draft(), origin: " " }, { ...draft(), idempotencyKey: {} },
    { ...attempted, lease: { owner: "tab", expiresAt: "never" } },
    { ...attempted, lease: { owner: "", expiresAt: 100 } }, { ...attempted, lease: undefined },
    { ...accepted, accepted: undefined }, { ...accepted, accepted: { interactionId: " " } },
    { ...accepted, accepted: { interactionId: "i", inputFrameId: 4 } },
    { ...attempted, accepted: accepted.accepted },
  ]) expect(() => validateConsoleSendAttempt(malformed as never)).toThrow();
  for (const interactionId of [123, " ", null]) {
    expect(() => finishConsoleSendAttempt(attempted, { state: "accepted", interactionId } as never)).toThrow(/prove acceptance/);
  }
});
it("keeps authoritative acceptance terminal across late failures", () => {
  const attempted = beginConsoleSendAttempt(draft(), { owner: "tab", now: 2, handlingMode: "queue" });
  const accepted = finishConsoleSendAttempt(attempted, { state: "accepted", interactionId: "owner-receipt", inputFrameId: "frame" });
  expect(finishConsoleSendAttempt(accepted, { state: "outcome-unknown", error: "late timeout" })).toBe(accepted);
  expect(finishConsoleSendAttempt(accepted, { state: "definitely-rejected", error: "late refusal" })).toBe(accepted);
});
