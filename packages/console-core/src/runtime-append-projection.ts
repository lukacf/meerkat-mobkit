import type { ConsoleFrame } from "./runtime-types";

type RecordValue = Record<string, unknown>;
type Origin = { session_id: string; run_id: string; input_id: string; append_ordinal: number };
type Position = Array<string | number>;
type ProjectedFrame = {
  frame: ConsoleFrame;
  scope: string | null;
  position?: Position;
  sequence?: number;
  origin?: Origin;
  canonical?: boolean;
  observedThrough?: number;
  settled?: boolean;
};
type NoticeSnapshot = {
  frame: ConsoleFrame;
  cursor: number;
  observedThrough: number;
  notices: Array<{ offset: number; message: RecordValue; origin: Origin }>;
  settled: Set<string>;
  historyPositions?: Map<string, Position>;
};

function record(value: unknown): RecordValue | null {
  return value && typeof value === "object" && !Array.isArray(value) ? value as RecordValue : null;
}

function identifier(value: unknown): value is string {
  return typeof value === "string" && value.length > 0 && value.trim() === value;
}

function ordinal(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

const string = (value: unknown): value is string => typeof value === "string";
const boolean = (value: unknown): value is boolean => typeof value === "boolean";
const optional = (value: unknown, check: (value: unknown) => boolean) => value == null || check(value);
const defaulted = (value: unknown, check: (value: unknown) => boolean) => value === undefined || check(value);
const strings = (value: unknown) => Array.isArray(value) && value.every(string);
const oneOf = (value: unknown, values: readonly string[]) => string(value) && values.includes(value);
const uuid = (value: unknown) => string(value)
  && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(value);
const optionalStrings = (value: RecordValue, keys: string[]) => keys.every(key => optional(value[key], string));
const operation = (value: unknown) => oneOf(value, ["add", "remove", "reload"]);
const phase = (value: unknown) => oneOf(value, ["pending", "applied", "draining", "forced", "failed"]);

// Validate the Core wire shape before a complete image can invalidate existing
// rows. Core's SystemNoticeBlock decoder preserves unknown block tags, while
// known tags and SystemNoticeKind use their typed contracts.
function contentBlock(value: unknown): boolean {
  const block = record(value);
  if (!block) return false;
  switch (block.type) {
    case "text": return string(block.text);
    case "image": return string(block.media_type) && (block.source === "inline" ? string(block.data)
      : block.source === "blob" && string(block.blob_id));
    case "video": return string(block.media_type) && ordinal(block.duration_ms)
      && (block.source === "inline" ? string(block.data) : block.source === "uri" && string(block.uri));
    case "structured": return Object.hasOwn(block, "data");
    case "skill_context": {
      const key = record(block.skill_key);
      return string(block.text) && !!key && uuid(key.source_uuid) && string(key.skill_name)
        && /^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(key.skill_name);
    }
    default: return false;
  }
}

function toolConfigStatus(value: unknown): boolean {
  const status = record(value);
  if (!status) return false;
  switch (status.kind) {
    case "boundary_applied": return boolean(status.base_changed) && boolean(status.visible_changed) && ordinal(status.revision);
    case "deferred_catalog_delta": return ordinal(status.added_hidden_count)
      && ordinal(status.removed_hidden_count) && ordinal(status.pending_source_count);
    case "warning_failed_closed": return string(status.error);
    case "external_tool_delta": return phase(status.phase) && optional(status.detail, string);
    default: return false;
  }
}

function toolConfig(value: unknown): boolean {
  const payload = record(value);
  return !!payload && operation(payload.operation) && string(payload.target) && boolean(payload.persisted)
    && toolConfigStatus(payload.status_info)
    && optional(payload.applied_at_turn, value => ordinal(value) && value <= 0xffff_ffff)
    && optional(payload.domain, value => oneOf(value, ["tool_scope", "deferred_catalog"]))
    && optional(payload.deferred_catalog_delta, value => {
      const delta = record(value);
      return !!delta && ["added_hidden_names", "removed_hidden_names", "pending_sources"]
        .every(key => defaulted(delta[key], strings));
    });
}

function noticeBlock(value: unknown): boolean {
  const block = record(value);
  if (!block || !string(block.type)) return false;
  const content = (value: unknown) => Array.isArray(value) && value.every(contentBlock);
  switch (block.type) {
    case "comms": return string(block.kind) && oneOf(block.direction, ["incoming", "outgoing", "internal"])
      && optional(block.peer, value => {
        const peer = record(value);
        return !!peer && uuid(peer.id) && optional(peer.display_name, string);
      })
      && optional(block.sender_taint, value => oneOf(value, ["clean", "tainted"]))
      && optionalStrings(block, ["request_id", "intent", "status", "summary"])
      && defaulted(block.content, content);
    case "external_event": return string(block.source) && string(block.event_type)
      && optionalStrings(block, ["summary", "body"]) && defaulted(block.content, content);
    case "tool_config": return toolConfig(block.payload);
    case "mcp": return optionalStrings(block, ["server_id", "detail"])
      && optional(block.operation, operation) && optional(block.phase, phase)
      && defaulted(block.persisted, boolean) && defaulted(block.pending_sources, strings);
    case "background_job": return string(block.job_id)
      && oneOf(block.status, ["completed", "failed", "aborted", "cancelled", "retired", "terminated"])
      && optionalStrings(block, ["display_name", "detail"]) && defaulted(block.persisted, boolean);
    case "auth": return string(block.state) && optionalStrings(block, ["binding", "detail"]);
    case "runtime_notice": return string(block.category) && optional(block.detail, string);
    case "unknown": return optional(block.summary, string);
    default: return true;
  }
}

function canonicalNotice(message: RecordValue): boolean {
  return message.role === "system_notice"
    && oneOf(message.kind, ["generic", "comms", "external_event", "mcp_pending", "mcp", "background_job",
      "tool_scope", "tool_scope_warning", "auth_reauth_required"])
    && optional(message.body, string)
    && defaulted(message.blocks, value => Array.isArray(value) && value.every(noticeBlock))
    && string(message.created_at) && Number.isFinite(Date.parse(message.created_at));
}

function consoleCursor(value: unknown): number | null {
  if (typeof value !== "string" || !/^console:\d+$/.test(value)) return null;
  const cursor = Number(value.slice(8));
  return ordinal(cursor) ? cursor : null;
}

function originOf(message: RecordValue | null): Origin | null {
  const origin = record(message?.runtime_origin);
  return origin && identifier(origin.session_id) && identifier(origin.run_id)
    && identifier(origin.input_id) && ordinal(origin.append_ordinal)
    ? origin as Origin : null;
}

function scopeOf(frame: ConsoleFrame): string | null {
  return identifier(frame.runtimeKey) && identifier(frame.sessionId)
    ? JSON.stringify([frame.runtimeKey, frame.sessionId]) : null;
}

function logicalKey(frame: ConsoleFrame, origin: Origin): string {
  return `runtime-notice:${JSON.stringify([
    frame.runtimeKey, frame.sessionId, origin.session_id, origin.input_id, origin.append_ordinal,
  ])}`;
}

/** Exact notice identity also scopes older comms de-duplication callbacks. */
export function runtimeAppendNoticeKey(frame: ConsoleFrame): string | null {
  if (frame.event !== "system_notice" || !scopeOf(frame)) return null;
  const origin = originOf(record(record(frame.data)?.message));
  return origin ? logicalKey(frame, origin) : null;
}

function attemptKey(scope: string, runId: string, inputId: string): string {
  return JSON.stringify([scope, runId, inputId]);
}

function noticeSnapshot(frame: ConsoleFrame): NoticeSnapshot | null {
  const data = record(frame.data), scope = scopeOf(frame);
  const cursor = consoleCursor(frame.cursor), observedThrough = consoleCursor(data?.observed_through);
  if (frame.event !== "runtime_notice_snapshot" || frame.sourceKind !== "session_history"
    || !scope || data?.session_id !== frame.sessionId || data.complete !== true
    || cursor === null || observedThrough === null || observedThrough >= cursor
    || !Array.isArray(data.notices) || !Array.isArray(data.settled_attempts)) return null;
  const notices: NoticeSnapshot["notices"] = [];
  const offsets = new Set<number>(), identities = new Set<string>();
  for (const value of data.notices) {
    const row = record(value), message = record(row?.message), origin = originOf(message);
    if (!row || !ordinal(row.offset) || !message || !origin
      || !canonicalNotice(message)) return null;
    const key = logicalKey(frame, origin);
    if (offsets.has(row.offset) || identities.has(key)) return null;
    offsets.add(row.offset);
    identities.add(key);
    notices.push({ offset: row.offset, message, origin });
  }
  const settled = new Set<string>();
  for (const value of data.settled_attempts) {
    const attempt = record(value);
    if (!attempt || !identifier(attempt.run_id) || !identifier(attempt.input_id)) return null;
    settled.add(attemptKey(scope, attempt.run_id, attempt.input_id));
  }
  let historyPositions: NoticeSnapshot["historyPositions"];
  if (data.history_positions !== undefined) {
    if (!Array.isArray(data.history_positions)) return null;
    historyPositions = new Map();
    for (const value of data.history_positions) {
      const row = record(value);
      const position = positionFromCursor(frame.sessionId!, row?.source_cursor);
      if (!row || !identifier(row.frame_id) || !position || historyPositions.has(row.frame_id)) return null;
      historyPositions.set(row.frame_id, position);
    }
  }
  return { frame, cursor, observedThrough, notices, settled, historyPositions };
}

function positionFromCursor(sessionId: string, sourceCursor: unknown): Position | undefined {
  if (!string(sourceCursor) || !sourceCursor.startsWith(`${sessionId}:`)) return undefined;
  const parts = sourceCursor.slice(sessionId.length + 1).split(":");
  if (!/^\d+$/.test(parts[0]) || !ordinal(Number(parts[0]))) return undefined;
  if (parts.some(part => /^\d+$/.test(part) ? !ordinal(Number(part)) : !/^[a-zA-Z_][a-zA-Z0-9_-]*$/.test(part))) return undefined;
  return parts.map(part => /^\d+$/.test(part) ? Number(part) : part);
}

function canonicalPosition(frame: ConsoleFrame): Position | undefined {
  return frame.sourceKind === "session_history" && identifier(frame.sessionId)
    ? positionFromCursor(frame.sessionId, frame.sourceCursor) : undefined;
}

function sourceSequence(frame: ConsoleFrame): number | undefined {
  const value = record(frame.data)?.source_sequence;
  return frame.sourceKind === "console_event" && ordinal(value) ? value : undefined;
}

function comparePosition(left: Position, right: Position): number {
  for (let index = 0; index < Math.min(left.length, right.length); index++) {
    const a = left[index], b = right[index];
    if (a === b) continue;
    if (typeof a === "number" && typeof b === "number") return a - b;
    if (typeof a !== typeof b) return typeof a === "number" ? -1 : 1;
    return String(a).localeCompare(String(b));
  }
  return left.length - right.length;
}

function projected(frame: ConsoleFrame): ProjectedFrame {
  return { frame, scope: scopeOf(frame), position: canonicalPosition(frame), sequence: sourceSequence(frame) };
}

function newestObservation(nodes: ProjectedFrame[], sourceOrder: boolean): ProjectedFrame | undefined {
  if (sourceOrder) {
    const sequenced = nodes.filter(node => node.sequence !== undefined);
    if (sequenced.length) {
      const attempts = new Set(sequenced.map(node => node.origin!.run_id));
      const latest = sequenced.reduce((latest, node) => Math.max(latest, node.sequence!), 0);
      // Compare the strongest source witnesses first. A legacy replay of an
      // already sequenced attempt cannot create a cyclic recency comparison.
      return newestObservation(nodes.filter(node => node.sequence === latest
        || (node.sequence === undefined && !attempts.has(node.origin!.run_id))), false);
    }
  }
  return nodes.reduce<ProjectedFrame | undefined>((previous, node) => {
    if (!previous) return node;
    const cursor = node.observedThrough ?? consoleCursor(node.frame.cursor);
    const previousCursor = previous.observedThrough ?? consoleCursor(previous.frame.cursor);
    return cursor !== null && (previousCursor === null || cursor > previousCursor) ? node : previous;
  }, undefined);
}

function toolCounterpartKey(item: ProjectedFrame): string | null {
  if (!item.scope) return null;
  const call = ["tool_call_requested", "tool_call", "tool_execution_started"].includes(item.frame.event);
  const result = ["tool_result_received", "tool_execution_completed"].includes(item.frame.event);
  if (!call && !result) return null;
  const data = record(item.frame.data);
  const id = data?.tool_call_id ?? data?.id;
  return identifier(id) ? JSON.stringify([item.scope, call ? "call" : "result", id]) : null;
}

function userCounterpartKey(item: ProjectedFrame): string | null {
  const id = item.frame.interactionId;
  return item.scope && item.frame.event === "user_input" && typeof id === "string"
    && /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(id)
    ? JSON.stringify([item.scope, "user", id]) : null;
}

// Compute a stable linear extension of two owner orders, never a comparator that
// sometimes compares timestamps and sometimes offsets. The original order is
// the tie-breaker for frames for which neither source supplies a relationship.
function orderBySource(nodes: ProjectedFrame[]): ConsoleFrame[] {
  const canonicalCounterparts = new Map<string, Position>();
  for (const node of nodes) {
    const key = toolCounterpartKey(node) ?? userCounterpartKey(node);
    if (key && node.position && node.frame.sourceKind === "session_history") canonicalCounterparts.set(key, node.position);
  }
  for (const node of nodes) {
    const key = toolCounterpartKey(node) ?? userCounterpartKey(node);
    if (!node.position && key && node.frame.sourceKind === "console_event") node.position = canonicalCounterparts.get(key);
  }
  const scopes = new Map<string, number[]>();
  nodes.forEach((node, index) => {
    if (!node.scope) return;
    const indices = scopes.get(node.scope) ?? [];
    indices.push(index);
    scopes.set(node.scope, indices);
  });
  const edges = nodes.map(() => new Set<number>());
  const incoming = nodes.map(() => 0);
  const connect = (from: number, to: number) => {
    if (from === to || edges[from].has(to)) return;
    edges[from].add(to);
    incoming[to]++;
  };
  const liveFirst = (a: number, b: number) => (
    Number(nodes[a].frame.sourceKind === "session_history") - Number(nodes[b].frame.sourceKind === "session_history")
    || a - b
  );
  for (const indices of scopes.values()) {
    const positioned = indices.filter(index => nodes[index].position)
      .sort((a, b) => comparePosition(nodes[a].position!, nodes[b].position!) || liveFirst(a, b));
    for (let index = 1; index < positioned.length; index++) connect(positioned[index - 1], positioned[index]);
    const sequenced = indices.filter(index => nodes[index].sequence !== undefined)
      .sort((a, b) => nodes[a].sequence! - nodes[b].sequence! || a - b);
    for (let index = 1; index < sequenced.length; index++) connect(sequenced[index - 1], sequenced[index]);
  }
  // Min heap keeps unconnected source rows in their existing stable order and
  // avoids quadratic scans during a long-running transcript's render pass.
  const ready: number[] = [];
  const push = (value: number) => {
    let index = ready.length;
    ready.push(value);
    while (index > 0) {
      const parent = (index - 1) >> 1;
      if (ready[parent] <= value) break;
      ready[index] = ready[parent];
      index = parent;
    }
    ready[index] = value;
  };
  const pop = () => {
    const value = ready[0];
    const tail = ready.pop()!;
    if (ready.length) {
      let index = 0;
      while (index * 2 + 1 < ready.length) {
        let child = index * 2 + 1;
        if (child + 1 < ready.length && ready[child + 1] < ready[child]) child++;
        if (ready[child] >= tail) break;
        ready[index] = ready[child];
        index = child;
      }
      ready[index] = tail;
    }
    return value;
  };
  incoming.forEach((count, index) => { if (!count) push(index); });
  const ordered: ConsoleFrame[] = [];
  while (ready.length) {
    const index = pop();
    ordered.push(nodes[index].frame);
    for (const next of edges[index]) if (--incoming[next] === 0) push(next);
  }
  // Conflicting history images are not authority to reorder a transcript. The
  // backend's complete-current-image projection resolves those observations.
  return ordered.length === nodes.length ? ordered : nodes.map(node => node.frame);
}

/** Project exact runtime appends over the caller's existing stable frame order. */
export function reconcileRuntimeAppendFrames(frames: readonly ConsoleFrame[]): ConsoleFrame[] {
  const snapshots = new Map<string, NoticeSnapshot>();
  for (const frame of frames) {
    const snapshot = noticeSnapshot(frame);
    if (!snapshot) continue;
    const scope = scopeOf(frame)!;
    const previous = snapshots.get(scope);
    if (!previous || snapshot.cursor > previous.cursor) snapshots.set(scope, snapshot);
  }
  const discarded = new Set<string>();
  for (const frame of frames) {
    if (frame.event !== "boundary_appends_discarded" || frame.sourceKind !== "console_event") continue;
    const data = record(frame.data), scope = scopeOf(frame);
    if (!scope || data?.session_id !== frame.sessionId || data?.run_id !== frame.runId
      || !identifier(data?.run_id) || !Array.isArray(data?.input_ids)) continue;
    for (const input of data.input_ids) if (identifier(input)) discarded.add(attemptKey(scope, data.run_id, input));
  }

  const nodes: ProjectedFrame[] = [];
  const candidates = new Map<string, ProjectedFrame[]>();
  const append = (node: ProjectedFrame) => {
    nodes.push(node);
    if (!node.origin) return;
    const key = logicalKey(node.frame, node.origin);
    const twins = candidates.get(key) ?? [];
    twins.push(node);
    candidates.set(key, twins);
  };
  for (const frame of frames) {
    if (frame.event === "boundary_appends_discarded" || frame.event === "runtime_notice_snapshot") continue;
    const data = record(frame.data), scope = scopeOf(frame);
    const snapshot = scope ? snapshots.get(scope) : undefined;
    const cursor = consoleCursor(frame.cursor);
    const observed = snapshot && cursor !== null && cursor <= snapshot.observedThrough;
    if (frame.event === "boundary_append_applied" && Array.isArray(data?.notices) && data.notices.length) {
      if (frame.sourceKind !== "console_event" || !scope || !identifier(frame.runId)
        || data.run_id !== frame.runId || !identifier(data.input_id)
        || !ordinal(data.append_count) || !ordinal(data.transcript_start)) continue;
      for (const value of data.notices) {
        const message = record(value), origin = originOf(message);
        if (!message || !origin || origin.session_id !== frame.sessionId || origin.run_id !== frame.runId
          || origin.input_id !== data.input_id || origin.append_ordinal >= data.append_count
          || !ordinal(data.transcript_start + origin.append_ordinal)) continue;
        const timestamp = typeof message.created_at === "string" ? Date.parse(message.created_at) : NaN;
        const noticeFrame: ConsoleFrame = {
          ...frame, id: logicalKey(frame, origin), event: "system_notice",
          timestampMs: Number.isFinite(timestamp) ? timestamp : frame.timestampMs,
          data: { message: { ...message, role: "system_notice" } },
        };
        append({ frame: noticeFrame, scope, origin,
          position: [data.transcript_start + origin.append_ordinal], sequence: sourceSequence(frame),
          settled: Boolean(observed && snapshot.settled.has(attemptKey(scope, origin.run_id, origin.input_id))) });
      }
      continue;
    }
    const node = projected(frame);
    if (observed && snapshot.historyPositions && frame.sourceKind === "session_history") {
      node.position = snapshot.historyPositions.get(frame.id);
    }
    if (frame.event === "system_notice" && frame.sourceKind === "session_history" && scope) {
      const message = record(data?.message), origin = originOf(message);
      if (origin) {
        if (observed) continue;
        node.origin = origin;
        node.canonical = true;
        node.frame = { ...frame, id: logicalKey(frame, origin) };
      }
    }
    append(node);
  }
  for (const [scope, snapshot] of snapshots) {
    for (const { offset, message, origin } of snapshot.notices) {
      const frame: ConsoleFrame = {
        ...snapshot.frame, event: "system_notice", id: logicalKey(snapshot.frame, origin),
        runId: origin.run_id, sourceCursor: `${snapshot.frame.sessionId}:${offset}`,
        timestampMs: Date.parse(String(message.created_at)), data: { message },
      };
      append({ frame, scope, origin, position: [offset], canonical: true, observedThrough: snapshot.observedThrough });
    }
  }

  const chosen = new Map<string, ProjectedFrame>();
  for (const [key, twins] of candidates) {
    const canonical = newestObservation(twins.filter(node => node.canonical), false);
    const eligible = twins.filter(node => !node.settled
      && !discarded.has(attemptKey(node.scope!, node.origin!.run_id, node.origin!.input_id)));
    const winner = canonical ?? newestObservation(eligible, true);
    if (!winner) continue;
    // Positive history proves this exact attempt remains current, including its
    // source order. Settlement or a delayed discard cannot erase that anchor.
    const liveTwin = newestObservation((canonical ? twins : eligible).filter(node => !node.canonical
      && node.origin!.run_id === winner.origin!.run_id), true);
    if (liveTwin?.sequence !== undefined) winner.sequence = liveTwin.sequence;
    chosen.set(key, winner);
  }
  const emitted = new Set<string>();
  const reconciled: ProjectedFrame[] = [];
  for (const node of nodes) {
    if (!node.origin) { reconciled.push(node); continue; }
    const key = logicalKey(node.frame, node.origin);
    if (emitted.has(key)) continue;
    emitted.add(key);
    const winner = chosen.get(key);
    if (winner) reconciled.push(winner);
  }
  return orderBySource(reconciled);
}
