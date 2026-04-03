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
}

export interface ConversationMessageEntry extends ConversationTimelineEntryBase {
  kind: "message";
  variant: "plain" | "rich" | "meta";
  text?: string;
  blocks?: ConversationRichBlock[];
  richStyle?: "default" | "streaming";
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

export type ConversationTimelineEntry = ConversationMessageEntry | ConversationSummaryEntry;

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
