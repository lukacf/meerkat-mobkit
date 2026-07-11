import {
  conversationRichBlockHasCopyAction,
  conversationRichBlocksToText,
  type ConversationRichBlock,
} from "./rich-content";

export type ConversationRole = "assistant" | "user" | "system" | "other";
export type ConversationPresentation = "assistant" | "user" | "participant" | "system";

export interface ConversationTone {
  variables?: Record<string, string> | null;
}

export interface ConversationIdentity {
  id: string;
  label: string;
  role: ConversationRole;
  kind?: string | null;
  meta?: string | null;
  presentation?: ConversationPresentation;
  showLabel?: boolean;
  avatarLabel?: string | null;
  tone?: ConversationTone | null;
}

export interface ConversationEmptySuggestion {
  id: string;
  label: string;
  value: string;
  iconName?: string | null;
}

export interface ConversationEmptyStateSpec {
  title: string;
  subtitle: string;
  projectLabel?: string | null;
  iconName?: string | null;
  suggestions?: ConversationEmptySuggestion[];
}

interface ConversationTimelineEntryBase {
  id: string;
  identity: ConversationIdentity;
  createdAt?: string;
  copyText?: string;
  /** Source frame's interaction id. UUID-form ids are authoritative for
   * live/history twin identity (mobkit 0.7.30, meerkat ask 15 addendum);
   * legacy `console-interaction-*` strings are not comparable across
   * live/history sources. */
  interactionId?: string;
}

export interface ConversationMessageEntry extends ConversationTimelineEntryBase {
  kind: "message";
  variant: "plain" | "rich" | "meta";
  text?: string;
  blocks?: ConversationRichBlock[];
  richStyle?: "default" | "streaming";
  /**
   * Typed host-task metadata. System tasks stay distinct from operator turns
   * all the way through the shared renderer instead of masquerading as user
   * messages.
   */
  taskKind?: string;
  taskLabel?: string;
  taskId?: string;
  taskStatus?: string;
  runId?: string | null;
}

export interface ConversationSummaryFile {
  name: string;
  plus: number;
  minus: number;
}

export interface ConversationSummaryEntry extends ConversationTimelineEntryBase {
  kind: "summary";
  title: string;
  plus: number;
  minus: number;
  files: ConversationSummaryFile[];
  actionLabel?: string | null;
}

export type FlowRunStatus = "idle" | "queued" | "running" | "cancelling" | "completed" | "failed" | "stopped";

export interface ConversationFlowRunMemberRow {
  memberKey: string;
  label: string;
  caption: string;
  status: FlowRunStatus;
  tone?: ConversationTone | null;
  // A prebuilt view of this member's own streamed transcript (tool cards,
  // thinking, code) rendered when the row is expanded. Absent until the live
  // execution snapshot has hydrated the member's sub-transcript.
  subView?: ConversationViewState | null;
}

// A flow run (a helper-mob crew spawned on demand) rendered inline in the
// conversation as one card, replacing the deleted watch rail. Rows are the crew
// members with honest live status — there is no fabricated step ordering because
// flow definitions carry no ordered-step model.
export interface ConversationFlowRunEntry extends ConversationTimelineEntryBase {
  kind: "flow_run";
  helperId: string;
  flowName: string;
  objective?: string | null;
  status: FlowRunStatus;
  outcome?: string | null;
  rows: ConversationFlowRunMemberRow[];
  // True when the crew exists only as persisted records (no live execution
  // snapshot) and can be brought back — the card offers a Resume action.
  restorable?: boolean;
}

// Aggregate posture of one goal/root work-item card. "mixed" is a fully
// terminal graph that ended in a blend of completed/cancelled outcomes.
export type WorkGraphCardStatus = "active" | "blocked" | "completed" | "failed" | "mixed";

export interface ConversationWorkGraphItemRow {
  itemId: string;
  title: string;
  // Upstream WorkStatus wire value: open|in_progress|blocked|completed|cancelled|failed.
  status: string;
  priority?: string | null;
  ownerLabel?: string | null;
  // CAS token — operator actions must send the latest observed revision.
  // Absent means no frame ever carried one: actions must resolve it from the
  // service before mutating (never guess 0).
  revision?: number;
  depth: number;
  parentId?: string | null;
  // Upstream allows multiple parents per child; the row is placed under its
  // first observed parent (parentId) and any further parents are listed here
  // (parent titles, or ids when the parent was never observed) as an
  // "also under …" note in the row detail.
  alsoUnder?: string[];
  blocked?: boolean;
  dueAt?: string | null;
  lastEventAt?: string | null;
  description?: string | null;
  labels?: string[];
  evidence?: string[];
  createdAt?: string | null;
  updatedAt?: string | null;
}

export interface ConversationWorkGraphAttentionRow {
  bindingId: string;
  // pursue|coordinate|review|falsify|judge|observe
  mode: string;
  // Human form of the binding status: "active", "paused until …", …
  statusLabel: string;
  targetLabel?: string | null;
  // Binding machine revision (CAS token for attention mutations).
  revision?: number;
  itemId?: string | null;
}

// One evolving in-conversation card per goal/root work item, aggregated from
// the turn's workgraph tool-call frames. Rebuilt from scratch on every adapter
// pass (same posture as tool blocks) with a stable `workgraph:{rootId}` id so
// live updates land in place.
export interface ConversationWorkGraphEntry extends ConversationTimelineEntryBase {
  kind: "workgraph";
  rootId: string;
  // Stable UI-state anchor. The entry `id` migrates when a loose item grows
  // a hierarchy (catch-all `workgraph:interaction:{id}` → rooted
  // `workgraph:{rootId}`), remounting the card; this key stays pinned to the
  // first contributing interaction PLUS the first item id folded into this
  // specific card, so collapse/expansion state survives the rekey moment
  // without bleeding between cards born in the same interaction. Cards that
  // never folded an item anchor on an "unrooted" placeholder segment.
  uiStateKey?: string;
  title: string;
  objective?: string | null;
  status: WorkGraphCardStatus;
  progress: { completed: number; total: number };
  items: ConversationWorkGraphItemRow[];
  // Items hidden by the per-card render cap (the most recently active rows
  // stay in `items`). Hidden items still count toward `progress` and the
  // card status; the card renders one "+N more items" overflow row for them.
  itemOverflowCount?: number;
  attention: ConversationWorkGraphAttentionRow[];
  recentEvents?: string[];
  // True when the most recent workgraph tool call folded into this card
  // failed — the card shows a subtle failure indicator (failures never
  // resurrect generic tool rows).
  lastActionFailed?: boolean;
  lastUpdatedAt?: string;
}

export type ConversationTimelineEntry =
  | ConversationMessageEntry
  | ConversationSummaryEntry
  | ConversationFlowRunEntry
  | ConversationWorkGraphEntry;

export interface ConversationTimelineGroup {
  id: string;
  identity: ConversationIdentity;
  entries: ConversationTimelineEntry[];
  copyText?: string;
}

export interface ConversationTurnDiffLine {
  type: "context" | "add" | "remove";
  text: string;
  oldLine: number | null;
  newLine: number | null;
}

export interface ConversationTurnDiffHunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: ConversationTurnDiffLine[];
}

export interface ConversationTurnDiffFile {
  path: string;
  plus: number;
  minus: number;
  hunks: ConversationTurnDiffHunk[];
}

export interface ConversationTurnDiff {
  fileCount: number;
  plus: number;
  minus: number;
  capturedAt?: string;
  files: ConversationTurnDiffFile[];
}

export interface ConversationViewState {
  conversationId: string;
  title?: string | null;
  entries: ConversationTimelineEntry[];
  groups: ConversationTimelineGroup[];
  turnDiff: ConversationTurnDiff | null;
  emptyState: ConversationEmptyStateSpec | null;
}

export function conversationIdentityPresentation(
  identity: ConversationIdentity | null | undefined,
): ConversationPresentation {
  if (identity?.presentation) {
    return identity.presentation;
  }
  if (identity?.role === "user") {
    return "user";
  }
  if (identity?.role === "system") {
    return "system";
  }
  if (identity?.role === "other") {
    return "participant";
  }
  return "assistant";
}

export function conversationIdentityShowsLabel(
  identity: ConversationIdentity | null | undefined,
): boolean {
  if (!identity?.label) {
    return false;
  }
  if (typeof identity.showLabel === "boolean") {
    return identity.showLabel;
  }
  const presentation = conversationIdentityPresentation(identity);
  return presentation === "participant" || presentation === "system";
}

export function conversationIdentityGroupKey(
  identity: ConversationIdentity | null | undefined,
): string {
  if (!identity) {
    return "unknown:assistant:hidden";
  }

  return [
    identity.id || "unknown",
    conversationIdentityPresentation(identity),
    conversationIdentityShowsLabel(identity) ? "label" : "hidden",
  ].join(":");
}

export function conversationEntryText(entry: ConversationTimelineEntry): string {
  if (entry.kind === "summary") {
    const fileLines = entry.files
      .map((file) => `${file.name} +${file.plus} -${file.minus}`)
      .join("\n");
    return [entry.title, fileLines].filter(Boolean).join("\n");
  }

  if (entry.kind === "flow_run") {
    const rowLines = entry.rows.map((row) => `${row.label}: ${row.caption}`);
    return [entry.flowName, entry.objective || "", ...rowLines, entry.outcome || ""]
      .filter(Boolean)
      .join("\n");
  }

  if (entry.kind === "workgraph") {
    const itemLines = entry.items.map((item) => (
      `${"  ".repeat(item.depth)}${item.title} — ${item.status.replace(/_/g, " ")}`
    ));
    const attentionLines = entry.attention.map((row) => (
      `${row.mode}: ${row.statusLabel}${row.targetLabel ? ` → ${row.targetLabel}` : ""}`
    ));
    return [
      `${entry.title} (${entry.progress.completed}/${entry.progress.total})`,
      entry.objective || "",
      ...itemLines,
      ...attentionLines,
    ].filter(Boolean).join("\n");
  }

  return String(entry.copyText || entry.text || conversationRichBlocksToText(entry.blocks)).trim();
}

export function conversationMessageHasIntrinsicCopyAction(
  entry: ConversationTimelineEntry,
): boolean {
  if (entry.kind !== "message" || entry.variant !== "rich") {
    return false;
  }

  return Boolean(entry.blocks?.some((block) => conversationRichBlockHasCopyAction(block)));
}

export function groupConversationTimelineEntries(
  entries: ConversationTimelineEntry[],
): ConversationTimelineGroup[] {
  const groups: ConversationTimelineGroup[] = [];

  for (const entry of entries) {
    const current = groups.at(-1);
    if (
      !current
      || conversationIdentityGroupKey(current.identity) !== conversationIdentityGroupKey(entry.identity)
    ) {
      groups.push({
        id: `${entry.identity.id}-${entry.id}`,
        identity: entry.identity,
        entries: [entry],
        copyText: conversationEntryText(entry),
      });
      continue;
    }

    current.entries.push(entry);
    const nextCopyText = conversationEntryText(entry);
    current.copyText = [current.copyText, nextCopyText].filter(Boolean).join("\n\n");
  }

  return groups;
}
