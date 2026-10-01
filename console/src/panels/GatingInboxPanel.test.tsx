import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { GatingInboxPanel } from "./GatingInboxPanel";
import { normalizePendingApproval, type PendingApprovalSnapshot } from "../../../packages/console-core/src/pending-approvals";

afterEach(cleanup);
const REF = "gpr1.0000000000000000000000000000000a.1";
const request = normalizePendingApproval({ pending_ref: REF, pending_id: "p1", action_id: "a1", action: "Deploy release", rationale: "Publish reviewed changes" })!;
const snapshot = (status: PendingApprovalSnapshot["status"]): PendingApprovalSnapshot => ({ scopeKey: "scope", status, requests: [request], decisions: {}, readOnly: false });
describe("GatingInboxPanel shared resource", () => {
  it("does not mistake unavailable data for an empty inbox", () => {
    const view = render(<GatingInboxPanel pending={[]} audit={[]} onDecide={vi.fn()} resource={{ ...snapshot("unavailable"), requests: [] }} />);
    expect(view.getByText("Approvals unavailable")).toBeTruthy();
    expect(view.queryByText("No pending approvals.")).toBeNull();
    view.rerender(<GatingInboxPanel pending={[]} audit={[]} onDecide={vi.fn()} resource={{ ...snapshot("ready"), requests: [] }} />);
    expect(view.getByText("No pending approvals.")).toBeTruthy();
  });
  it("navigates attention to the exact request without a second fetch owner", () => {
    const view = render(<GatingInboxPanel pending={[]} audit={[]} onDecide={vi.fn()} resource={snapshot("ready")} selectedPendingRef={REF} />);
    const selected = view.container.querySelector<HTMLElement>(`[data-approval-id="${REF}"]`);
    expect(selected?.dataset.selected).toBe("true");
    expect(document.activeElement).toBe(selected);
    expect(view.getByText("Publish reviewed changes")).toBeTruthy();
  });
  it.each([
    { name: "an empty audit", audit: [] },
    { name: "recorded decisions", audit: [
      { audit_id: "audit-approve", action_id: "deploy-release", decision: "approve" },
      { audit_id: "audit-reject", action_id: "delete-database", decision: "reject" },
      { audit_id: "audit-escalate", action_id: "grant-access", decision: "escalate" },
    ] },
  ])("does not manufacture policy counts or rules from $name", ({ audit }) => {
    const view = render(<GatingInboxPanel pending={[]} audit={audit} onDecide={vi.fn()} resource={snapshot("ready")} />);
    expect(view.container.querySelector(".gating__head")?.textContent).not.toMatch(/\d+ polic/);
    fireEvent.click(view.getByRole("button", { name: "Policies", exact: true }));
    expect(view.getByRole("status").textContent).toBe("Policy details are not available in this console.");
    for (const invented of ["scope: *", "active", "paused", "Auto on low risk", "High rejection rate"]) {
      expect(view.queryByText(invented, { exact: true })).toBeNull();
    }
    expect(view.queryByText(/No .*policies/)).toBeNull();
    expect(view.queryByText("deploy-release", { exact: true })).toBeNull();
    expect(view.queryByText("delete-database", { exact: true })).toBeNull();
    expect(view.queryByText("grant-access", { exact: true })).toBeNull();
  });
  it("preserves pending selection and audit access after viewing unavailable policies", () => {
    const audit = [
      { audit_id: "auto-approval", action_id: "automatic-check", decision: "auto_approve" },
      { audit_id: "manual-approval", action_id: "manual-review", decision: "approve" },
    ];
    const view = render(<GatingInboxPanel pending={[]} audit={audit} onDecide={vi.fn()} resource={snapshot("ready")} selectedPendingRef={REF} />);
    fireEvent.click(view.getByTestId("gating-tab:policies"));
    fireEvent.click(view.getByTestId("gating-tab:audit"));
    expect(view.getByText("automatic-check", { exact: true })).toBeTruthy();
    expect(view.getByText("manual-review", { exact: true })).toBeTruthy();
    fireEvent.click(view.getByTestId("gating-tab:auto"));
    expect(view.getByText("automatic-check", { exact: true })).toBeTruthy();
    expect(view.queryByText("manual-review", { exact: true })).toBeNull();
    fireEvent.click(view.getByTestId("gating-tab:pending"));
    expect(view.getByText("Publish reviewed changes")).toBeTruthy();
    expect(document.activeElement).toBe(view.container.querySelector(`[data-approval-id="${REF}"]`));
  });
  it("explains an unsupported approval capability without claiming access was denied", () => {
    const view = render(<GatingInboxPanel pending={[]} audit={[]} onDecide={vi.fn()} resource={{ ...snapshot("unsupported"), requests: [], readOnly: true }} />);
    expect(view.getByText("Approvals are not available for this connection")).toBeTruthy();
    expect(view.queryByText("Approval access denied")).toBeNull();
    expect(view.queryByText("No pending approvals.")).toBeNull();
  });
});
