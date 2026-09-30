import type { ConsoleFrame } from "../types";

/** One input that contributed to a stopped member run. */
export interface RunStopContributor {
  input_id: string;
  completion: string;
  terminal?: string | null;
}

/**
 * meerkat's typed run-stop receipt, relayed verbatim by
 * `mobkit/stop_member_run`.
 */
export type RunStopReceipt =
  | { outcome: "stopped"; run_id: string; contributors: RunStopContributor[] }
  | { outcome: "not_current"; run_id: string; current_run_id?: string | null }
  | { outcome: "not_stoppable"; run_id: string; state: string };

function frameRunId(frame: ConsoleFrame): string | null {
  const direct = frame.runId?.trim();
  if (direct) return direct;
  if (frame.data && typeof frame.data === "object") {
    const identity = (frame.data as Record<string, unknown>).identity;
    if (identity && typeof identity === "object") {
      const runId = (identity as Record<string, unknown>).run_id;
      if (typeof runId === "string" && runId.trim()) return runId.trim();
    }
  }
  return null;
}

function isSteerDelivery(frame: ConsoleFrame): boolean {
  return (
    frame.event === "interaction_complete" &&
    !!frame.data &&
    typeof frame.data === "object" &&
    (frame.data as Record<string, unknown>).reason === "steer_delivered"
  );
}

function isRunTerminal(frame: ConsoleFrame): boolean {
  switch (frame.event) {
    case "run_completed":
    case "run_failed":
    case "interaction_failed":
      return true;
    case "interaction_complete":
      return !isSteerDelivery(frame);
    default:
      return false;
  }
}

/**
 * The run id of the member's in-flight run, read from the timeline: the latest
 * `run_started` (its `identity.run_id`) that no terminal frame has closed yet.
 * `null` when no run is known to be active, so the console never offers to
 * stop a run it cannot name.
 */
export function activeRunIdFromFrames(frames: readonly ConsoleFrame[]): string | null {
  let active: string | null = null;
  for (const frame of frames) {
    if (frame.event === "run_started") {
      const runId = frameRunId(frame);
      if (runId) active = runId;
      continue;
    }
    if (active && isRunTerminal(frame)) {
      const runId = frameRunId(frame);
      if (!runId || runId === active) active = null;
    }
  }
  return active;
}

/** Validate the `mobkit/stop_member_run` result; fail closed on anything else. */
export function parseRunStopResult(result: unknown): RunStopReceipt {
  const receipt =
    result && typeof result === "object"
      ? (result as Record<string, unknown>).receipt
      : undefined;
  if (!receipt || typeof receipt !== "object") {
    throw new Error("invalid mobkit/stop_member_run result: missing receipt");
  }
  const record = receipt as Record<string, unknown>;
  if (typeof record.run_id !== "string") {
    throw new Error("invalid mobkit/stop_member_run receipt: missing run_id");
  }
  switch (record.outcome) {
    case "stopped":
      if (
        !Array.isArray(record.contributors) ||
        !record.contributors.every(
          (row) =>
            !!row &&
            typeof row === "object" &&
            typeof (row as Record<string, unknown>).input_id === "string" &&
            typeof (row as Record<string, unknown>).completion === "string",
        )
      ) {
        throw new Error("invalid mobkit/stop_member_run receipt: malformed contributors");
      }
      break;
    case "not_current":
      break;
    case "not_stoppable":
      if (typeof record.state !== "string") {
        throw new Error("invalid mobkit/stop_member_run receipt: missing state");
      }
      break;
    default:
      throw new Error(`invalid mobkit/stop_member_run receipt outcome: ${String(record.outcome)}`);
  }
  return receipt as RunStopReceipt;
}

/** Operator-facing summary of a run-stop receipt. */
export function describeRunStopReceipt(receipt: RunStopReceipt): string {
  switch (receipt.outcome) {
    case "stopped": {
      const cancelled = receipt.contributors.filter((row) => row.terminal === "cancelled").length;
      const consumed = receipt.contributors.length - cancelled;
      const parts = [`${cancelled} input${cancelled === 1 ? "" : "s"} cancelled`];
      if (consumed > 0) parts.push(`${consumed} kept`);
      return `Run stopped: ${parts.join(", ")}.`;
    }
    case "not_current":
      return "That run already ended; nothing was stopped.";
    case "not_stoppable":
      return `The run cannot be stopped right now (runtime ${receipt.state}).`;
  }
}
