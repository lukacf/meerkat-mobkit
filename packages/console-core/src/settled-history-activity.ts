import type { ConsoleFrame } from "./runtime-types";
import {
  assistantHistorySnapshot,
  assistantMessageCursorSequence,
} from "./assistant-message-identity";

// These historical observations describe work, not an open run reservation.
const LEGACY_ACTIVITY = new Set([
  "reasoning_delta", "reasoning_complete", "tool_call_requested", "tool_call",
  "tool_execution_started", "tool_result_received", "tool_execution_completed", "server_tool_content",
]);

function exactScope(frame: ConsoleFrame): string | undefined {
  const fields = [frame.runtimeKey, frame.identity, frame.sessionId];
  return fields.every(value => typeof value === "string" && value.trim())
    ? JSON.stringify(fields) : undefined;
}

/**
 * A settled history observation can cover old historical activity without
 * manufacturing a run terminal. Explicit run/interaction state and later live
 * activity remain the lifecycle owner's responsibility.
 */
export function settledHistoryActivity(frames: readonly ConsoleFrame[]): ReadonlySet<ConsoleFrame> {
  type Observation = { cursor: bigint; through: bigint; ids: ReadonlySet<string>; conflict: boolean };
  const observations = new Map<string, Observation>();
  for (const frame of frames) {
    const snapshot = assistantHistorySnapshot(frame);
    const scope = exactScope(frame);
    const cursor = assistantMessageCursorSequence(frame.cursor);
    if (!snapshot || !scope || cursor === undefined) continue;
    const previous = observations.get(scope);
    if (!previous || cursor > previous.cursor) {
      observations.set(scope, { cursor, through: snapshot.observedThrough,
        ids: snapshot.assistantMessageIds, conflict: false });
    } else if (cursor === previous.cursor && (snapshot.observedThrough !== previous.through
      || snapshot.assistantMessageIds.size !== previous.ids.size
      || [...snapshot.assistantMessageIds].some(id => !previous.ids.has(id)))) {
      previous.conflict = true;
    }
  }
  const covered = new Set<ConsoleFrame>();
  for (const frame of frames) {
    if (frame.sourceKind !== "session_history" || !LEGACY_ACTIVITY.has(frame.event)) continue;
    const scope = exactScope(frame);
    const observation = scope && observations.get(scope);
    const cursor = assistantMessageCursorSequence(frame.cursor);
    if (observation && !observation.conflict && cursor !== undefined && cursor <= observation.through) {
      covered.add(frame);
    }
  }
  return covered;
}
