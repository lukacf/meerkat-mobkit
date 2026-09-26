import { approvalMatchesConversation, type ApprovalAction, type PendingApprovalSnapshot } from "../../../console-core/src/pending-approvals";
import { ApprovalCard } from "./approval-card";

export type ConversationApprovalProps = {
  approvalSnapshot?: PendingApprovalSnapshot;
  /** Canonical host-authorized target, never a label or actor ID. */
  approvalIdentity?: string;
  onApprovalDecision?: (pendingId: string, action: ApprovalAction) => void | Promise<void>;
};

export function ConversationApprovals({ approvalSnapshot, approvalIdentity, onApprovalDecision, conversationId, interactionIds }: ConversationApprovalProps & {
  conversationId?: string;
  /** Omit for conversation-only provenance, supply exact IDs for turn placement. */
  interactionIds?: readonly string[];
}) {
  if (!approvalSnapshot || !approvalIdentity) return null;
  const requests = approvalSnapshot.requests.filter((request) =>
    Boolean(request.origin?.interactionId) === Boolean(interactionIds)
    && approvalMatchesConversation(request, { identity: approvalIdentity, conversationId, interactionIds }));
  return <>{requests.map((request) => <ApprovalCard key={request.pendingId} request={request} resourceStatus={approvalSnapshot.status} decision={approvalSnapshot.decisions[request.pendingId]} readOnly={approvalSnapshot.readOnly || !onApprovalDecision} onDecide={onApprovalDecision ?? (() => {})} />)}</>;
}
