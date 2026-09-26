import { render, fireEvent, cleanup } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApprovalAttention, ApprovalCard } from "./approval-card";
import { normalizePendingApproval, type PendingApprovalSnapshot } from "../../../console-core/src/pending-approvals";

afterEach(cleanup);
const request = normalizePendingApproval({ pending_id: "request:1", action_id: "action:1", action: "Publish release artifacts", rationale: "Make the tested release available", actor_id: "actor", deadline_at_ms: 1, payload: { destination: "production", full: "complete request contents" } })!;
const snapshot = (status: PendingApprovalSnapshot["status"] = "ready"): PendingApprovalSnapshot => ({ scopeKey: "authority", status, requests: [request], decisions: {}, readOnly: false });
describe("ApprovalCard", () => {
  it("shows readable scope, full request details and supported decisions", () => {
    const decide = vi.fn();
    const view = render(<ApprovalCard request={request} resourceStatus="ready" onDecide={decide} />);
    expect(view.getByText("Publish release artifacts")).toBeTruthy();
    expect(view.getByText("Make the tested release available")).toBeTruthy();
    expect(view.getByText("Complete request details")).toBeTruthy();
    expect(view.container.querySelector("pre")?.textContent).toContain("complete request contents");
    fireEvent.click(view.getByRole("button", { name: "Approve" }));
    expect(decide).toHaveBeenCalledWith("request:1", "approve");
    expect(view.getByText("Approval needed")).toBeTruthy();
  });
  it("disables all decisions while submitting, stale or read-only", () => {
    const decide = vi.fn();
    const view = render(<ApprovalCard request={request} resourceStatus="ready" onDecide={decide} decision={{ phase: "submitting", action: "approve" }} />);
    expect((view.getByRole("button", { name: "Approve" }) as HTMLButtonElement).disabled).toBe(true);
    view.rerender(<ApprovalCard request={request} resourceStatus="stale" onDecide={decide} />);
    expect((view.getByRole("button", { name: "Approve" }) as HTMLButtonElement).disabled).toBe(true);
    view.rerender(<ApprovalCard request={request} resourceStatus="ready" onDecide={decide} readOnly />);
    expect((view.getByRole("button", { name: "Approve" }) as HTMLButtonElement).disabled).toBe(true);
    expect(decide).not.toHaveBeenCalled();
  });
  it("keeps decision context visible and exact opaque scope available in the disclosure", () => {
    const scopedRequest = { ...request, origin: { identity: "router:main" }, riskTier: "r3" };
    const view = render(<ApprovalCard request={scopedRequest} resourceStatus="ready" onDecide={() => {}} />);
    const details = view.container.querySelector("details")!;
    expect(details.hasAttribute("open")).toBe(false);
    expect(details.textContent).toContain("request:1");
    expect(details.textContent).toContain("action:1");
    expect(view.getByText("Request").closest("details")).toBe(details);
    expect(view.getByText("Action scope").closest("details")).toBe(details);
    expect(details.querySelector("pre")?.textContent).toBe(JSON.stringify(request.raw, null, 2));
    expect(view.getByText("router:main").closest("details")).toBeNull();
    expect(view.getByText("r3").closest("details")).toBeNull();
    expect(view.container.querySelector("time")?.closest("details")).toBeNull();
    for (const action of ["approve", "reject", "escalate"]) {
      expect(view.getByTestId(`gating-action:request:1:${action}`).dataset.action).toBe(action);
    }
  });
  it("cannot treat a local deadline as expired and shows authoritative expiry only", () => {
    const view = render(<ApprovalCard request={request} resourceStatus="ready" onDecide={() => {}} />);
    expect(view.getByText("Approval needed")).toBeTruthy();
    view.rerender(<ApprovalCard request={{ ...request, status: "expired" }} resourceStatus="ready" onDecide={() => {}} />);
    expect(view.getByText("Expired")).toBeTruthy();
    expect(view.queryByRole("button", { name: "Approve" })).toBeNull();
  });
  it("attention has unavailable states instead of a false zero count and navigates to central request", () => {
    const onOpen = vi.fn();
    const view = render(<ApprovalAttention snapshot={{ ...snapshot("unavailable"), requests: [] }} onOpen={onOpen} />);
    expect(view.getByText("Approvals unavailable")).toBeTruthy();
    expect(view.queryByText("0 pending")).toBeNull();
    view.rerender(<ApprovalAttention snapshot={snapshot()} onOpen={onOpen} />);
    fireEvent.click(view.getByText("Publish release artifacts"));
    expect(onOpen).toHaveBeenCalledWith("request:1");
  });
});
