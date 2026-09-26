import * as React from "react";
import type { ApprovalAction, ApprovalDecisionState, PendingApproval, PendingApprovalSnapshot } from "../../../console-core/src/pending-approvals";

const labels: Record<ApprovalAction, string> = { approve: "Approve", reject: "Reject", escalate: "Escalate" };
export interface ApprovalCardProps {
  request: PendingApproval;
  resourceStatus: PendingApprovalSnapshot["status"];
  decision?: ApprovalDecisionState;
  readOnly?: boolean;
  onDecide(pendingId: string, action: ApprovalAction): void | Promise<void>;
}

/** The same request and decision owner can feed an inbox and several panes. */
export function ApprovalCard({ request, resourceStatus, decision, readOnly = false, onDecide }: ApprovalCardProps) {
  const pending = request.status === "pending" && decision?.phase !== "settled";
  const submitting = decision?.phase === "submitting";
  const stale = resourceStatus !== "ready";
  const state = submitting ? "submitting" : decision?.phase === "failed" ? "failed" : request.status === "expired" ? "expired" : !pending ? "settled" : stale ? "stale" : "pending";
  return (
    <article className="cc-approval" data-state={state} data-testid={`gating-pending:${request.pendingId}`}>
      <header className="cc-approval__header">
        <strong>{request.action}</strong>
        <span className="cc-approval__status" role="status">{state === "pending" ? "Approval needed" : state === "submitting" ? "Submitting decision" : state === "stale" ? "Approval state may be out of date" : state === "failed" ? "Decision unconfirmed" : state === "expired" ? "Expired" : "Resolved"}</span>
      </header>
      {request.rationale ? <p>{request.rationale}</p> : null}
      <dl className="cc-approval__scope">
        {request.origin ? <><dt>Origin</dt><dd>{request.origin.identity}</dd></> : null}
        {request.riskTier ? <><dt>Risk</dt><dd>{request.riskTier}</dd></> : null}
        {request.deadlineAtMs !== undefined ? <><dt>Deadline</dt><dd><time dateTime={new Date(request.deadlineAtMs).toISOString()}>{new Date(request.deadlineAtMs).toLocaleString()}</time></dd></> : null}
      </dl>
      <details className="cc-approval__details"><summary>Complete request details</summary>
        <dl className="cc-approval__scope">
          <dt>Request</dt><dd><code>{request.pendingId}</code></dd>
          <dt>Action scope</dt><dd><code>{request.actionId}</code></dd>
        </dl>
        <pre>{JSON.stringify(request.raw, null, 2)}</pre>
      </details>
      {decision?.error ? <p role="alert">{decision.error}</p> : null}
      {decision?.result?.next_pending_id ? <p>Escalated to <code>{decision.result.next_pending_id}</code></p> : null}
      {readOnly ? <p>Read-only access</p> : null}
      {resourceStatus === "forbidden" ? <p>Approval access denied</p> : null}
      {pending ? <div className="cc-approval__actions">{request.actions.map((action) => (
        <button key={action} type="button" disabled={readOnly || stale || submitting} data-action={action} data-testid={`gating-action:${request.pendingId}:${action}`} onClick={() => { void onDecide(request.pendingId, action); }}>{labels[action]}</button>
      ))}</div> : null}
    </article>
  );
}

export function ApprovalAttention({ snapshot, onOpen }: {
  snapshot: PendingApprovalSnapshot;
  onOpen(pendingId?: string): void;
}) {
  const requests = snapshot.requests.filter((request) => request.status === "pending" && snapshot.decisions[request.pendingId]?.phase !== "settled");
  const count = snapshot.status === "ready" ? `${requests.length} pending` : snapshot.status === "forbidden" ? "Access denied" : snapshot.status === "loading" ? "Checking approvals" : snapshot.status === "stale" ? "Approvals may be out of date" : "Approvals unavailable";
  return (
    <section className="cc-approval-attention" aria-label="Needs you" data-testid="approval-attention">
      <button type="button" onClick={() => onOpen()}><strong>Needs you</strong><span role="status">{count}</span></button>
      {requests.map((request) => <button type="button" key={request.pendingId} data-testid={`approval-attention:${request.pendingId}`} onClick={() => onOpen(request.pendingId)}>{request.action}</button>)}
    </section>
  );
}
