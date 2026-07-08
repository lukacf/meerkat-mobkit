import type { ConsoleDockTarget } from "./dock";

export type MobKitWorkbenchTarget =
  | MobKitIdentityChatTarget
  | MobKitIdentityInspectTarget
  | MobKitControlWorkbenchTarget;

export interface MobKitIdentityChatTarget extends ConsoleDockTarget {
  kind: "mobkit/identity-chat";
  identity: string;
  memberId?: string;
  addressingMode: "identity";
}

export interface MobKitIdentityInspectTarget extends ConsoleDockTarget {
  kind: "mobkit/identity-inspect";
  identity: string;
  memberId?: string;
}

export type MobKitControlTargetKind =
  | "mobkit/topology"
  | "mobkit/activity"
  | "mobkit/roster"
  | "mobkit/routing"
  | "mobkit/gating"
  | "mobkit/access"
  | "mobkit/memory"
  | "mobkit/workgraph"
  | "mobkit/logs";

export interface MobKitControlWorkbenchTarget extends ConsoleDockTarget {
  kind: MobKitControlTargetKind;
}

export interface HostWorkbenchTarget<TPayload = unknown> extends ConsoleDockTarget {
  kind: `${string}/${string}`;
  title: string;
  payloadVersion: number;
  payload?: TPayload;
  provenance: "host";
}

export type ConsoleWorkbenchTarget<THost = unknown> =
  | MobKitWorkbenchTarget
  | HostWorkbenchTarget<THost>;

type UnknownRecord = Record<string, unknown>;

const LEGACY_CONTROL_TARGETS: Record<string, MobKitControlTargetKind> = {
  topology: "mobkit/topology",
  health: "mobkit/activity",
  timeline: "mobkit/activity",
  roster: "mobkit/roster",
  routing: "mobkit/routing",
  gating: "mobkit/gating",
  gates: "mobkit/gating",
  access: "mobkit/access",
  memory: "mobkit/memory",
  workgraph: "mobkit/workgraph",
  logs: "mobkit/logs",
};

export function migrateConsoleWorkbenchTarget(
  input: unknown,
): ConsoleWorkbenchTarget | null {
  if (!isRecord(input)) {
    return null;
  }

  const id = stringValue(input.id);
  const kind = stringValue(input.kind);
  const title = stringValue(input.title);
  if (!id || !kind || !title) {
    return null;
  }

  if (kind === "agent-chat") {
    const identity = stringValue(input.identity) || stringValue(input.memberId) || id;
    return {
      ...baseTarget(input, id, "mobkit/identity-chat", title),
      identity,
      memberId: stringValue(input.memberId),
      addressingMode: "identity",
    };
  }

  if (kind === "identity-inspect") {
    const identity = stringValue(input.identity) || stringValue(input.memberId) || id.replace(/^inspect:/, "");
    return {
      ...baseTarget(input, id, "mobkit/identity-inspect", title),
      identity,
      memberId: stringValue(input.memberId),
    };
  }

  const controlKind = LEGACY_CONTROL_TARGETS[kind];
  if (controlKind) {
    return baseTarget(input, id, controlKind, title);
  }

  if (kind.startsWith("mobkit/")) {
    return normalizeMobKitWorkbenchTarget(input, id, kind, title);
  }

  if (isNamespacedKind(kind) && kind !== "mobkit/unknown") {
    const payloadVersion = typeof input.payloadVersion === "number" && Number.isSafeInteger(input.payloadVersion)
      ? input.payloadVersion
      : 1;
    return {
      ...baseTarget(input, id, kind as `${string}/${string}`, title),
      payloadVersion,
      payload: input.payload,
      provenance: "host",
    };
  }

  return null;
}

function normalizeMobKitWorkbenchTarget(
  input: UnknownRecord,
  id: string,
  kind: string,
  title: string,
): ConsoleWorkbenchTarget | null {
  if (kind === "mobkit/identity-chat") {
    const identity = stringValue(input.identity) || stringValue(input.memberId);
    if (!identity) return null;
    return {
      ...baseTarget(input, id, kind, title),
      identity,
      memberId: stringValue(input.memberId),
      addressingMode: "identity",
    };
  }

  if (kind === "mobkit/identity-inspect") {
    const identity = stringValue(input.identity) || stringValue(input.memberId);
    if (!identity) return null;
    return {
      ...baseTarget(input, id, kind, title),
      identity,
      memberId: stringValue(input.memberId),
    };
  }

  if (Object.values(LEGACY_CONTROL_TARGETS).includes(kind as MobKitControlTargetKind)) {
    return baseTarget(input, id, kind as MobKitControlTargetKind, title);
  }

  return null;
}

function baseTarget<TKind extends string>(
  input: UnknownRecord,
  id: string,
  kind: TKind,
  title: string,
): ConsoleDockTarget & { kind: TKind } {
  return {
    id,
    kind,
    title,
    subtitle: stringValue(input.subtitle),
    iconName: stringValue(input.iconName),
    badgeLabel: stringValue(input.badgeLabel),
  };
}

function isRecord(value: unknown): value is UnknownRecord {
  return Boolean(value && typeof value === "object" && !Array.isArray(value));
}

function stringValue(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value.trim() : undefined;
}

function isNamespacedKind(kind: string): kind is `${string}/${string}` {
  const [namespace, name, ...rest] = kind.split("/");
  return Boolean(namespace && name && rest.length === 0);
}
