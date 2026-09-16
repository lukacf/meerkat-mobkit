import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { queryVoiceAvailability } from "./voice-session";
import { useVoiceReadiness } from "./use-voice-readiness";

vi.mock("./voice-session", () => ({ queryVoiceAvailability: vi.fn() }));
const query = vi.mocked(queryVoiceAvailability);

beforeEach(() => {
  vi.useFakeTimers();
  query.mockReset();
  query.mockResolvedValue(true);
});
afterEach(() => vi.useRealTimers());

it("requires positive per-target readiness and separately retains the voice target", async () => {
  const view = renderHook(
    ({ focused, voice }) => useVoiceReadiness("/gateway", focused, voice, true),
    { initialProps: { focused: "alpha", voice: "alpha" } },
  );
  expect(view.result.current).toEqual({});
  await act(async () => {});
  expect(view.result.current).toEqual({ alpha: true });
  expect(query.mock.calls).toEqual([["/gateway", "alpha"]]);
  view.rerender({ focused: "beta", voice: "alpha" });
  expect(view.result.current).toEqual({});
  await act(async () => {});
  expect(view.result.current).toEqual({ alpha: true, beta: true });
});

it("refreshes auth for the ongoing voice target even when no chat is selected", async () => {
  const view = renderHook(() => useVoiceReadiness("/gateway", null, "alpha", true));
  await act(async () => {});
  expect(view.result.current.alpha).toBe(true);
  query.mockResolvedValue(false);
  await act(() => vi.advanceTimersByTimeAsync(15_000));
  expect(view.result.current.alpha).toBe(false);
});

it("does not transfer readiness across gateways or accept stale navigation responses", async () => {
  let finishAlpha: (available: boolean) => void = () => {};
  query.mockImplementation((_baseUrl, identity) => identity === "alpha"
    ? new Promise((resolve) => { finishAlpha = resolve; })
    : Promise.resolve(false));
  const view = renderHook(
    ({ baseUrl, focused }) => useVoiceReadiness(baseUrl, focused, null, true),
    { initialProps: { baseUrl: "/first", focused: "alpha" } },
  );
  view.rerender({ baseUrl: "/second", focused: "beta" });
  await act(async () => {});
  await act(async () => finishAlpha(true));
  expect(view.result.current).toEqual({ beta: false });
});

it("does not probe a read-only console and stops refreshing on unmount", async () => {
  const view = renderHook(
    ({ enabled }) => useVoiceReadiness("/gateway", "alpha", null, enabled),
    { initialProps: { enabled: false } },
  );
  await act(async () => {});
  expect(query).not.toHaveBeenCalled();
  view.rerender({ enabled: true });
  await act(async () => {});
  expect(query).toHaveBeenCalledOnce();
  view.unmount();
  await act(() => vi.advanceTimersByTimeAsync(30_000));
  expect(query).toHaveBeenCalledOnce();
});
