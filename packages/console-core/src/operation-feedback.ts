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

/** Display projection only. No decision is inferred from arbitrary tool prose. */
export function operationFeedbackFromFrame(frame: Frame): Feedback | null {
  const data = record(frame.data);
  if (!data) return null;
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
      if (record(JSON.parse(block.text))?.error === "operation_refused") return refused();
    } catch { /* Non-JSON tool output stays ordinary output. */ }
  }
  return null;
}
