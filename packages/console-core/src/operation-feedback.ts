import type { ConversationMessageEntry } from "./conversation";

type Feedback = NonNullable<ConversationMessageEntry["operationFeedback"]>;
type Frame = { event: string; data?: unknown };

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown> : null;
}

const refused = (): Feedback => ({
  kind: "permission-refused",
  title: "Permission denied",
  detail: "This action is not permitted for this request. The agent can continue with permitted work.",
});

const confinementDetails = {
  invalid_requirement: "The confinement requirement is invalid.",
  invalid_launch: "The confined process launch is invalid.",
  unsupported_requirement: "This backend does not support the required confinement.",
  backend_unavailable: "The required confinement backend is unavailable.",
  preparation_failed: "Confined process preparation failed.",
} as const satisfies Record<NonNullable<Feedback["confinementRefusal"]>, string>;

const hookReasons = {
  policy_violation: "Policy violation",
  safety_violation: "Safety violation",
  schema_violation: "Schema violation",
  timeout: "Timeout",
  runtime_error: "Runtime error",
} as const satisfies Record<NonNullable<Feedback["hookReasonCode"]>, string>;

function identifier(value: unknown): string | null {
  return typeof value === "string" && value.length > 0 ? value : null;
}

function confinementFeedback(value: unknown, toolCallId: string): Feedback | null {
  if (typeof value !== "string" || !Object.hasOwn(confinementDetails, value)) return null;
  const refusal = value as keyof typeof confinementDetails;
  return {
    kind: "confinement-refused",
    title: "Action could not start",
    detail: `${confinementDetails[refusal]} This action did not run. The agent can continue with other work.`,
    toolCallId,
    confinementRefusal: refusal,
  };
}

/** Display projection only. No decision is inferred from arbitrary tool prose. */
export function operationFeedbackFromFrame(frame: Frame): Feedback | null {
  const data = record(frame.data);
  if (!data) return null;
  if (frame.event === "hook_launch_refused" && data.point === "pre_tool_execution") {
    const toolCallId = identifier(data.tool_use_id);
    const hookId = identifier(data.hook_id);
    const reason = record(data.reason);
    if (!toolCallId || !hookId || !reason) return null;
    if (reason.reason_code === "confinement_refused") {
      return confinementFeedback(reason.refusal, toolCallId);
    }
    if (reason.reason_code === "execution_failed" && typeof reason.message === "string") {
      return {
        kind: "hook-launch-failed",
        title: "Hook could not start",
        detail: "The hook could not start, so this action did not run. The agent can continue with other work.",
        toolCallId,
        hookId,
      };
    }
    return null;
  }
  if (frame.event === "operation_observation_failed" && data.phase === "outcome"
      && typeof data.operation_id === "string" && data.operation_id.length > 0
      && data.operation_id.length <= 256) {
    return {
      kind: "audit-unavailable",
      title: "Audit update unavailable",
      detail: "The action's outcome could not be recorded. Its actual result is unchanged; do not repeat it based on this notice.",
      operationId: data.operation_id,
    };
  }
  if (frame.event === "system_notice") {
    const message = record(data.message) ?? data;
    if (Array.isArray(message.blocks) && message.blocks.some((value) => {
      const block = record(value);
      return block?.type === "runtime_notice" && block.category === "operation_refused"
        && record(block.payload)?.code === "operation_refused";
    })) return refused();
  }
  if ((frame.event === "tool_result_received" || frame.event === "tool_execution_completed")
      && data.is_error === true && Array.isArray(data.content) && data.content.length === 1) {
    const block = record(data.content[0]);
    if (block?.type !== "text" || typeof block.text !== "string") return null;
    try {
      const payload = record(JSON.parse(block.text));
      if (payload?.error === "operation_refused") return refused();
      const toolCallId = identifier(data.tool_call_id ?? data.id);
      if (!toolCallId) return null;
      const detail = record(payload?.data);
      if (payload?.error === "confinement_refused") {
        return confinementFeedback(detail?.refusal, toolCallId);
      }
      if (payload?.error === "hook_denied" && detail?.point === "pre_tool_execution") {
        const hookId = identifier(detail.hook_id);
        const reason = detail.reason_code;
        if (!hookId || typeof reason !== "string" || !Object.hasOwn(hookReasons, reason)) return null;
        const hookReasonCode = reason as keyof typeof hookReasons;
        return {
          kind: "hook-denied",
          title: "Action blocked by hook",
          detail: `A hook denied this action (${hookReasons[hookReasonCode]}). The agent can continue with other work.`,
          toolCallId,
          hookId,
          hookReasonCode,
        };
      }
    } catch { /* Non-JSON tool output stays ordinary output. */ }
  }
  return null;
}
