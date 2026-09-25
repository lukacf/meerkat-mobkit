import { createContext, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import type { ConversationRichBlock, ConversationRichToolCallBlock } from "@console-core";
import type { ConversationViewportKey } from "./scroll-controller";

export type ConversationDisplayLabels = {
  tools?: ReadonlyMap<string, string>;
  /** Populate only with already-authorized roster entries, keyed by exact ID. */
  peers?: ReadonlyMap<string, string>;
};
export function explicitDisplayLabel(id: string, labels?: ReadonlyMap<string, string>): string {
  return labels?.get(id)?.trim() || id;
}
export const ROUTINE_TOOL_NAMES: ReadonlySet<string> = new Set(["read_file", "list_files", "search", "glob", "grep", "ls", "read", "file_read", "directory_list"]);
export function isCompletedRoutineTool(block: ConversationRichBlock): block is ConversationRichToolCallBlock {
  return block.type === "tool-call"
    && ROUTINE_TOOL_NAMES.has(block.name)
    && !block.peerIncoming && !block.peerTarget && !block.peerIdentity && !block.peerBody
    && block.status === "success"
    && block.completionEvidence?.outcome === "success"
    && block.completionEvidence.toolCallId === block.toolCallId
    && (block.completionEvidence.source === "runtime-result" || block.completionEvidence.source === "session-history");
}
export function canFoldCompletedTools(blocks: readonly ConversationRichBlock[]): blocks is ConversationRichToolCallBlock[] {
  return blocks.length >= 2 && blocks.every(isCompletedRoutineTool);
}

/** Group only adjacent eligible records, without rewriting their contents or IDs. */
export function groupRoutineToolRows<T>(rows: readonly T[], blocksFor: (row: T) => readonly ConversationRichBlock[] | undefined): { rows: T[]; tools: ConversationRichToolCallBlock[] }[] {
  const groups: { rows: T[]; tools: ConversationRichToolCallBlock[] }[] = [];
  for (const row of rows) {
    const blocks = blocksFor(row);
    const eligible = Boolean(blocks?.length && blocks.every(isCompletedRoutineTool));
    const previous = groups.at(-1);
    if (eligible && previous?.tools.length) {
      previous.rows.push(row); previous.tools.push(...blocks as ConversationRichToolCallBlock[]);
    } else {
      groups.push({ rows: [row], tools: eligible ? [...blocks!] as ConversationRichToolCallBlock[] : [] });
    }
  }
  return groups;
}

const scopes = new Map<string, Map<string, boolean>>();
function scopeState(key: string): Map<string, boolean> {
  const state = scopes.get(key) ?? new Map<string, boolean>();
  scopes.delete(key); scopes.set(key, state);
  if (scopes.size > 100) scopes.delete(scopes.keys().next().value!);
  return state;
}
const PresentationContext = createContext<{ labels?: ConversationDisplayLabels; disclosures: Map<string, boolean>; autoFold: boolean } | null>(null);
const FoldedToolsContext = createContext(false);
export function useInsideCompletedToolDisclosure() { return useContext(FoldedToolsContext); }
export function ConversationPresentationProvider({ labels, viewportKey, autoFold = true, children }: { labels?: ConversationDisplayLabels; viewportKey?: ConversationViewportKey; autoFold?: boolean; children: ReactNode }) {
  const local = useRef(new Map<string, boolean>());
  const oldAuthority = useRef(viewportKey?.authority);
  const key = viewportKey ? JSON.stringify([viewportKey.authority, viewportKey.identity, viewportKey.conversation, viewportKey.pane]) : null;
  const disclosures = useMemo(() => key ? scopeState(key) : local.current, [key]);
  useEffect(() => {
    if (oldAuthority.current !== viewportKey?.authority) {
      for (const existing of scopes.keys()) if (JSON.parse(existing)[0] === oldAuthority.current) scopes.delete(existing);
      local.current.clear();
      oldAuthority.current = viewportKey?.authority;
    }
  }, [viewportKey?.authority]);
  return <PresentationContext.Provider value={{ labels, disclosures, autoFold }}>{children}</PresentationContext.Provider>;
}
export function useConversationDisplayLabels() { return useContext(PresentationContext)?.labels; }

/** A layout disclosure only: the full underlying records and copy actions remain. */
export function CompletedToolDisclosure({ blocks, children }: { blocks: ConversationRichToolCallBlock[]; children: ReactNode }) {
  const context = useContext(PresentationContext);
  const local = useRef(new Map<string, boolean>());
  const disclosures = context?.disclosures ?? local.current;
  const key = JSON.stringify(blocks.map((block) => block.toolCallId));
  const initiallyOpen = disclosures.get(key) ?? context?.autoFold === false;
  const [state, setState] = useState(() => ({ disclosures, key, open: initiallyOpen }));
  const open = state.disclosures === disclosures && state.key === key ? state.open : initiallyOpen;
  return <details className="cc-completed-tools" open={open} onToggle={(event) => {
    const next = event.currentTarget.open;
    setState({ disclosures, key, open: next });
    disclosures.delete(key); disclosures.set(key, next);
    if (disclosures.size > 100) disclosures.delete(disclosures.keys().next().value!);
  }}>
    <summary>{blocks.length} completed tool calls</summary>
    <FoldedToolsContext.Provider value={true}><div className="cc-completed-tools__body">{children}</div></FoldedToolsContext.Provider>
  </details>;
}

export type ConversationHeaderProps = { title: string; identity: string; detail?: string | null; actions?: ReactNode; variant?: "full" | "compact" };
export function ConversationHeader({ title, identity, detail, actions, variant = "full" }: ConversationHeaderProps) {
  return <header className={`cc-conversation-header cc-conversation-header--${variant}`}>
    <div className="cc-conversation-header__target" title={identity}>
      <span className="cc-conversation-header__title">{title}</span>
      {variant === "full" ? <span className="cc-conversation-header__identity">{identity}{detail ? ` · ${detail}` : ""}</span> : null}
    </div>
    {actions ? <div className="cc-conversation-header__actions">{actions}</div> : null}
  </header>;
}
