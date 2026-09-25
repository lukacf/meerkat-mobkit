import { describe, expect, test } from "./test-support/vitest-shim";
import type { ConversationMessageEntry, ConversationTimelineEntry } from "./conversation";
import {
  decodeMemberAlias,
  describeConversationEntrySource,
  entryOriginFromFrameData,
  parseMemberCommsName,
  runtimeEventFromFrame,
  runtimeEventText,
  transcriptDayKey,
  transcriptDayLabel,
} from "./transcript-source";

const USER = { id: "user", label: "You", role: "user" as const };
const AGENT = { id: "triage", label: "Triage", role: "assistant" as const };
const SYSTEM = { id: "system", label: "System", role: "system" as const };

function message(overrides: Partial<ConversationMessageEntry>): ConversationTimelineEntry {
  return {
    kind: "message",
    id: "entry",
    identity: USER,
    variant: "plain",
    text: "",
    ...overrides,
  } as ConversationTimelineEntry;
}

// The payload exactly as meerkat's generic runtime event carries it.
const PEER_INGESTED = {
  kind: "message",
  peer: {
    display_name: "homecore/triage/mk--triage_cmain",
    id: "978419a8-69f6-5103-8d31-4482e1f76b52",
  },
  sender_taint: "tainted",
  source_event_type: "peer_content_ingested",
  type: "peer_content_ingested",
};

describe("transcript entry source classification", () => {
  test("assistant entries are labelled Assistant with the agent name as detail", () => {
    const source = describeConversationEntrySource(message({ identity: AGENT, text: "hi" }));
    expect(source.kind).toBe("assistant");
    expect(source.label).toBe("Assistant");
    expect(source.detail).toBe("Triage");
    expect(source.untrusted).toBe(false);
  });

  test("a user entry with no typed origin is a plain User message, whatever its text says", () => {
    const source = describeConversationEntrySource(message({
      text: "Operator gate probe. Reply with exactly the token gate-turn-proof-1 and nothing else.",
    }));
    expect(source.kind).toBe("user");
    expect(source.label).toBe("User message");
    expect(source.detail).toBe(null);
  });

  test("the console composer origin namespace reads as Operator", () => {
    const source = describeConversationEntrySource(message({ origin: { sendOrigin: "console:panel-7" } }));
    expect(source.kind).toBe("operator");
    expect(source.label).toBe("Operator");
  });

  test("any other send origin stays a User message and names the caller", () => {
    const source = describeConversationEntrySource(message({ origin: { sendOrigin: "homecore-gate" } }));
    expect(source.kind).toBe("user");
    expect(source.label).toBe("User message");
    expect(source.detail).toBe("via homecore-gate");
  });

  test("a persisted render class labels history user messages", () => {
    const cases: Array<[string, string, string]> = [
      ["external_event", "external_event", "External event"],
      ["flow_step", "flow_step", "Flow step"],
      ["peer_message", "peer_message", "Peer message"],
      ["peer_request", "peer_message", "Peer request"],
      ["continuation", "continuation", "Continuation"],
      ["ops_progress", "system_notice", "Progress update"],
    ];
    for (const [renderClass, kind, label] of cases) {
      const source = describeConversationEntrySource(message({ origin: { renderClass } }));
      expect(source.kind).toBe(kind);
      expect(source.label).toBe(label);
    }
  });

  test("user_prompt render class falls through to the send origin", () => {
    const source = describeConversationEntrySource(message({
      origin: { renderClass: "user_prompt", sendOrigin: "console:p" },
    }));
    expect(source.label).toBe("Operator");
  });

  test("typed host tasks are labelled by their task label", () => {
    const source = describeConversationEntrySource(message({
      identity: SYSTEM,
      taskKind: "scheduled_job",
      taskLabel: "Nightly digest",
    }));
    expect(source.kind).toBe("system_task");
    expect(source.label).toBe("Nightly digest");
  });

  test("peer_content_ingested reads as a sentence naming the decoded peer, flagged untrusted", () => {
    const runtimeEvent = runtimeEventFromFrame("peer_content_ingested", PEER_INGESTED);
    expect(runtimeEvent.peer).toEqual({
      id: "978419a8-69f6-5103-8d31-4482e1f76b52",
      displayName: "homecore/triage/mk--triage_cmain",
    });
    expect(runtimeEvent.senderTaint).toBe("tainted");
    const source = describeConversationEntrySource(message({
      identity: SYSTEM,
      variant: "meta",
      runtimeEvent,
    }));
    expect(source.kind).toBe("peer_message");
    expect(source.label).toBe("Message from triage:main");
    expect(source.sentence).toBe("Received a message from triage:main (homecore mob).");
    expect(source.untrusted).toBe(true);
  });

  test("roster labels replace the decoded alias when the console knows the peer", () => {
    const runtimeEvent = runtimeEventFromFrame("peer_content_ingested", PEER_INGESTED);
    const source = describeConversationEntrySource(
      message({ identity: SYSTEM, variant: "meta", runtimeEvent }),
      { resolvePeerLabel: (alias) => (alias === "triage:main" ? "Triage" : null) },
    );
    expect(source.sentence).toBe("Received a message from Triage (homecore mob).");
  });

  test("clean peer content is not flagged", () => {
    const runtimeEvent = runtimeEventFromFrame("peer_content_ingested", { ...PEER_INGESTED, sender_taint: "clean" });
    expect(describeConversationEntrySource(message({ identity: SYSTEM, variant: "meta", runtimeEvent })).untrusted)
      .toBe(false);
  });

  test("other runtime events read as a humanized sentence and never inline their payload", () => {
    const runtimeEvent = runtimeEventFromFrame("budget_warning", { remaining: 3, detail: { nested: true } });
    const text = runtimeEventText(runtimeEvent);
    expect(text).toBe("Budget warning.");
    const source = describeConversationEntrySource(message({ identity: SYSTEM, variant: "meta", runtimeEvent, text }));
    expect(source.kind).toBe("runtime_event");
    expect(source.label).toBe("System event");
    expect(source.sentence).toBe("Budget warning.");
  });

  test("a runtime event's typed message field becomes its detail", () => {
    const runtimeEvent = runtimeEventFromFrame("interaction_failed", { error: "peer response undeliverable" });
    expect(runtimeEventText(runtimeEvent)).toBe("Interaction failed: peer response undeliverable");
  });
});

describe("typed provenance from frame payloads", () => {
  test("reads the console send origin and the persisted render class", () => {
    expect(entryOriginFromFrameData({ content: "hi", origin: "console:p1" })).toEqual({ sendOrigin: "console:p1" });
    expect(entryOriginFromFrameData({
      content: "tick",
      message: { role: "user", render_metadata: { class: "external_event", salience: "normal" } },
    })).toEqual({ renderClass: "external_event" });
    expect(entryOriginFromFrameData({ content: "hi" })).toBe(null);
  });
});

describe("member comms names", () => {
  test("parses exactly three identifier-safe components", () => {
    expect(parseMemberCommsName("homecore/identity/mk--identity_cparent-1")).toEqual({
      mobId: "homecore",
      role: "identity",
      member: "mk--identity_cparent-1",
    });
    expect(parseMemberCommsName("a/b")).toBe(null);
    expect(parseMemberCommsName("a/b/c/d")).toBe(null);
    expect(parseMemberCommsName("a/b/c d")).toBe(null);
  });

  test("decodes the MobKit member id codec", () => {
    expect(decodeMemberAlias("mk--identity_cparent-1")).toBe("identity:parent-1");
    expect(decodeMemberAlias("mk--rt_creview_csingleton_c0")).toBe("rt:review:singleton:0");
    expect(decodeMemberAlias("mk--rt_cperson_cfederico_x2e_gomez_x40_king_x2e_com_c2"))
      .toBe("rt:person:federico.gomez@king.com:2");
    expect(decodeMemberAlias("mk--snake__case")).toBe("snake_case");
    expect(decodeMemberAlias("plain-member")).toBe("plain-member");
    // Not an encode production: returned unchanged.
    expect(decodeMemberAlias("mk--bad_q")).toBe("mk--bad_q");
    expect(decodeMemberAlias("mk--bad_x2e")).toBe("mk--bad_x2e");
  });
});

describe("day separators", () => {
  test("labels today, yesterday, and older days", () => {
    const now = new Date(2026, 8, 25, 10, 0, 0);
    expect(transcriptDayLabel("2026-09-25", now)).toBe("Today");
    expect(transcriptDayLabel("2026-09-24", now)).toBe("Yesterday");
    expect(transcriptDayLabel("2026-09-23", now)).toBe("Wednesday, 23 September 2026");
  });

  test("day keys are local calendar days", () => {
    const late = new Date(2026, 8, 24, 23, 59, 0).toISOString();
    const early = new Date(2026, 8, 25, 0, 1, 0).toISOString();
    expect(transcriptDayKey(late)).toBe("2026-09-24");
    expect(transcriptDayKey(early)).toBe("2026-09-25");
    expect(transcriptDayKey("not a date")).toBe(null);
  });
});
