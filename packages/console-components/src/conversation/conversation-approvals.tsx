import { approvalMatchesConversation, type ApprovalAction, type PendingApprovalSnapshot } from "../../../console-core/src/pending-approvals";
import { ApprovalCard } from "./approval-card";

export type ConversationApprovalProps = {
  approvalSnapshot?: PendingApprovalSnapshot;
  /** Canonical host-authorized target, never a label or actor ID. */
  approvalIdentity?: string;
  onApprovalDecision?: (pendingRef: string, action: ApprovalAction) => void | Promise<void>;
};

/** A continued interaction belongs beside its latest matching visible turn. */
export function approvalInteractionIdsByTurn(turns: readonly (readonly string[])[]): string[][] {
  const latest = new Map<string, number>();
  turns.forEach((ids, index) => ids.forEach((id) => latest.set(id, index)));
  return turns.map((ids, index) => [...new Set(ids)].filter((id) => latest.get(id) === index));
}

export function ConversationApprovals({ approvalSnapshot, approvalIdentity, onApprovalDecision, conversationId, interactionIds }: ConversationApprovalProps & {
  conversationId?: string;
  /** Omit for conversation-only provenance, supply exact IDs for turn placement. */
  interactionIds?: readonly string[];
}) {
  if (!approvalSnapshot || !approvalIdentity) return null;
  const requests = approvalSnapshot.requests.filter((request) =>
    Boolean(request.origin?.interactionId) === Boolean(interactionIds)
    && approvalMatchesConversation(request, { identity: approvalIdentity, conversationId, interactionIds }));
  return <>{requests.map((request) => <ApprovalCard key={request.pendingRef} request={request} resourceStatus={approvalSnapshot.status} decision={approvalSnapshot.decisions[request.pendingRef]} readOnly={approvalSnapshot.readOnly || !onApprovalDecision} onDecide={onApprovalDecision ?? (() => {})} />)}</>;
}
