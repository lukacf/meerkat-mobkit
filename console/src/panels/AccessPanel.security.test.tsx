import React from "react";
import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { AccessPanel, type AccessPreviewResult } from "./AccessPanel";

function props() {
  return {
    status: { available: true, enabled: true, revision: 1, subject: "admin@example.test", can_administer: true, actions: ["agent.view", "agent.send"] },
    config: { enabled: true, admins: ["admin@example.test"], rules: [], groups: {} },
    agents: [{ identity: "identity:agent", label: "Agent" }],
    onRefresh: vi.fn(), onSetEnabled: vi.fn(), onSaveAdmins: vi.fn(), onUpsertRule: vi.fn(),
    onDeleteRule: vi.fn(), onSaveGroup: vi.fn(), onDeleteGroup: vi.fn(),
    onPreview: vi.fn(async (): Promise<AccessPreviewResult> => ({ allowed: true })),
  };
}

describe("backend-owned access affordances", () => {
  it("does not infer administration permission from a loaded configuration", () => {
    const input = props();
    render(<AccessPanel {...input} status={{ ...input.status, can_administer: false }} />);
    expect(screen.queryByTestId("access-toggle-enabled")).toBeNull();
    expect(screen.queryByTestId("access-edit-admins")).toBeNull();
    expect(screen.queryByTestId("access-rule-new")).toBeNull();
    expect(screen.queryByText("admin@example.test")).toBeNull();
    expect(input.onSetEnabled).not.toHaveBeenCalled();
    expect(input.onUpsertRule).not.toHaveBeenCalled();
  });

  it("does not invent an action vocabulary when the owner supplied none", () => {
    const input = props();
    render(<AccessPanel {...input} status={{ ...input.status, actions: [] }} />);
    fireEvent.click(screen.getByTestId("access-tab:preview"));
    expect(screen.queryByRole("option", { name: "agent.view", exact: true })).toBeNull();
    fireEvent.change(screen.getByTestId("access-preview-subject"), { target: { value: "operator@example.test" } });
    expect(screen.getByTestId("access-preview-run")).toBeDisabled();
    expect(input.onPreview).not.toHaveBeenCalled();
  });

  it("invalidates a preview when its input changes, including an in-flight old answer", async () => {
    const input = props();
    let resolve!: (value: AccessPreviewResult) => void;
    input.onPreview = vi.fn(() => new Promise(done => { resolve = done; }));
    render(<AccessPanel {...input} />);
    fireEvent.click(screen.getByTestId("access-tab:preview"));
    const subject = screen.getByTestId("access-preview-subject");
    fireEvent.change(subject, { target: { value: "first@example.test" } });
    fireEvent.click(screen.getByTestId("access-preview-run"));
    expect(input.onPreview).toHaveBeenCalledWith("first@example.test", "agent.view", undefined);
    fireEvent.change(subject, { target: { value: "second@example.test" } });
    await act(async () => { resolve({ allowed: true, reason: "old answer" }); });
    expect(screen.queryByTestId("access-preview-result")).toBeNull();
  });

  it("retains only the server's current explicit action list", () => {
    const input = props();
    render(<AccessPanel {...input} status={{ ...input.status, actions: ["custom.current.action"] }} />);
    fireEvent.click(screen.getByTestId("access-tab:preview"));
    expect(screen.getByRole("option", { name: "custom.current.action" })).toBeVisible();
    expect(screen.queryByRole("option", { name: "access.admin", exact: true })).toBeNull();
    expect(screen.getByTestId("access-preview-action")).toHaveValue("custom.current.action");
  });
  it("invalidates completed and pending previews on owner revision refresh", async () => {
    const input = props();
    const view = render(<AccessPanel {...input} />);
    fireEvent.click(screen.getByTestId("access-tab:preview"));
    fireEvent.change(screen.getByTestId("access-preview-subject"), { target: { value: "reader" } });
    await act(async () => { fireEvent.click(screen.getByTestId("access-preview-run")); });
    expect(screen.getByTestId("access-preview-result")).toBeVisible();
    view.rerender(<AccessPanel {...input} status={{ ...input.status, revision: 2 }} />);
    expect(screen.queryByTestId("access-preview-result")).toBeNull();
  });

  it("preserves a failed rule draft and gates writes while owner state is stale", async () => {
    const input = props();
    input.onUpsertRule = vi.fn().mockRejectedValue(new Error("Save failed"));
    const view = render(<AccessPanel {...input} />);
    fireEvent.click(screen.getByTestId("access-tab:rules"));
    fireEvent.click(screen.getByTestId("access-rule-new"));
    fireEvent.change(screen.getByTestId("access-rule-id"), { target: { value: "keep-my-draft" } });
    await act(async () => { fireEvent.click(screen.getByTestId("access-rule-save")); });
    expect(screen.getByTestId("access-rule-id")).toHaveValue("keep-my-draft");
    view.rerender(<AccessPanel {...input} error="Owner refresh unavailable" />);
    expect(screen.getByTestId("access-rule-id")).toHaveValue("keep-my-draft");
    expect(screen.getByTestId("access-rule-save")).toBeDisabled();
    fireEvent.click(screen.getByTestId("access-rule-save"));
    expect(input.onUpsertRule).toHaveBeenCalledTimes(1);
  });

  it("does not expose loaded protected data when unavailable or authority is lost", () => {
    const input = props();
    const view = render(<AccessPanel {...input} />);
    expect(screen.getByText("admin@example.test")).toBeVisible();
    view.rerender(<AccessPanel {...input} status={{ ...input.status, available: false }} />);
    expect(screen.queryByText("admin@example.test")).toBeNull();
    expect(screen.queryByTestId("access-toggle-enabled")).toBeNull();
  });

  it("keeps read-only administration disabled without hiding authorized configuration", () => {
    const input = props();
    render(<AccessPanel {...input} readOnly />);
    expect(screen.getByText("admin@example.test")).toBeVisible();
    expect(screen.queryByTestId("access-toggle-enabled")).toBeNull();
  });

});

describe("preview decision and error scope", () => {
  it.each(["missing", "rejected"])("keeps %s decision unavailable without exposing private payload", async (kind) => {
    const input = props();
    input.onPreview = kind === "missing" ? vi.fn(async () => ({ reason: "PRIVATE_PREVIEW_ERROR" })) : vi.fn().mockRejectedValue(new Error("PRIVATE_PREVIEW_ERROR"));
    render(<AccessPanel {...input} />);
    fireEvent.click(screen.getByTestId("access-tab:preview"));
    fireEvent.change(screen.getByTestId("access-preview-subject"), { target: { value: "reader" } });
    await act(async () => { fireEvent.click(screen.getByTestId("access-preview-run")); });
    expect(screen.queryByTestId("access-preview-result")).toBeNull();
    expect(screen.getByTestId("access-preview-error")).toHaveTextContent("unavailable");
    expect(document.body).not.toHaveTextContent("PRIVATE_PREVIEW_ERROR");
    expect(screen.getByTestId("access-preview-run")).toBeEnabled();
  });
  it("ignores a rejected old preview after the authenticated subject changes", async () => {
    const input = props();
    let reject!: (error: Error) => void;
    input.onPreview = vi.fn(() => new Promise((_resolve, fail) => { reject = fail; }));
    const view = render(<AccessPanel {...input} />);
    fireEvent.click(screen.getByTestId("access-tab:preview"));
    fireEvent.change(screen.getByTestId("access-preview-subject"), { target: { value: "reader" } });
    fireEvent.click(screen.getByTestId("access-preview-run"));
    view.rerender(<AccessPanel {...input} status={{ ...input.status, subject: "next-admin" }} />);
    await act(async () => { reject(new Error("OLD_SCOPE_PRIVATE_ERROR")); });
    expect(screen.queryByTestId("access-preview-error")).toBeNull();
    expect(document.body).not.toHaveTextContent("OLD_SCOPE_PRIVATE_ERROR");
    expect(screen.getByTestId("access-preview-run")).toBeEnabled();
  });
});
