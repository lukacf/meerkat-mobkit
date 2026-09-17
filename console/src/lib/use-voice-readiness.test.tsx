import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { queryVoiceAvailability } from "./voice-session";
import { mergeVoiceReadiness, useVoiceReadiness, voiceReadinessDenied } from "./use-voice-readiness";

vi.mock("./voice-session", () => ({ queryVoiceAvailability: vi.fn() }));
const query = vi.mocked(queryVoiceAvailability);

beforeEach(() => {
  vi.useFakeTimers();
  query.mockReset();
  query.mockResolvedValue("available");
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
  query.mockResolvedValue("unavailable");
  await act(() => vi.advanceTimersByTimeAsync(15_000));
  expect(view.result.current.alpha).toBe(false);
});

it("keeps the last known readiness when a poll fails and flips only on a definite gateway answer", async () => {
  const view = renderHook(() => useVoiceReadiness("/gateway", null, "alpha", true));
  await act(async () => {});
  expect(view.result.current).toEqual({ alpha: true });
  query.mockResolvedValue("unknown");
  await act(() => vi.advanceTimersByTimeAsync(15_000));
  expect(view.result.current).toEqual({ alpha: true });
  expect(voiceReadinessDenied(view.result.current, "alpha")).toBe(false);
  await act(() => vi.advanceTimersByTimeAsync(15_000));
  expect(view.result.current).toEqual({ alpha: true });
  expect(query).toHaveBeenCalledTimes(3);
  query.mockResolvedValue("unavailable");
  await act(() => vi.advanceTimersByTimeAsync(15_000));
  expect(view.result.current).toEqual({ alpha: false });
  expect(voiceReadinessDenied(view.result.current, "alpha")).toBe(true);
});

it("never reports a never-confirmed identity as denied when its first poll fails", async () => {
  query.mockResolvedValue("unknown");
  const view = renderHook(() => useVoiceReadiness("/gateway", "alpha", null, true));
  await act(async () => {});
  expect(view.result.current).toEqual({});
  expect(view.result.current.alpha).toBeUndefined();
  expect(voiceReadinessDenied(view.result.current, "alpha")).toBe(false);
});

it("does not carry a known value across gateways when the new gateway's poll fails", async () => {
  const view = renderHook(
    ({ baseUrl }) => useVoiceReadiness(baseUrl, "alpha", null, true),
    { initialProps: { baseUrl: "/first" } },
  );
  await act(async () => {});
  expect(view.result.current).toEqual({ alpha: true });
  query.mockResolvedValue("unknown");
  view.rerender({ baseUrl: "/second" });
  await act(async () => {});
  expect(view.result.current).toEqual({});
});

it("does not transfer readiness across gateways or accept stale navigation responses", async () => {
  let finishAlpha: (available: "available" | "unavailable" | "unknown") => void = () => {};
  query.mockImplementation((_baseUrl, identity) => identity === "alpha"
    ? new Promise((resolve) => { finishAlpha = resolve; })
    : Promise.resolve("unavailable"));
  const view = renderHook(
    ({ baseUrl, focused }) => useVoiceReadiness(baseUrl, focused, null, true),
    { initialProps: { baseUrl: "/first", focused: "alpha" } },
  );
  view.rerender({ baseUrl: "/second", focused: "beta" });
  await act(async () => {});
  await act(async () => finishAlpha("available"));
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

it("merges poll rounds so only definite answers replace known values and denial needs an explicit false", () => {
  expect(mergeVoiceReadiness({}, [["alpha", "available"], ["beta", "unavailable"]])).toEqual({ alpha: true, beta: false });
  expect(mergeVoiceReadiness({ alpha: true, beta: false }, [["alpha", "unknown"], ["beta", "unknown"]]))
    .toEqual({ alpha: true, beta: false });
  expect(mergeVoiceReadiness({ alpha: true }, [["alpha", "unavailable"]])).toEqual({ alpha: false });
  expect(mergeVoiceReadiness({ alpha: true }, [["beta", "unknown"]])).toEqual({});
  expect(mergeVoiceReadiness({ alpha: true, stale: true }, [["alpha", "unknown"]])).toEqual({ alpha: true });
  expect(voiceReadinessDenied({ alpha: false }, "alpha")).toBe(true);
  expect(voiceReadinessDenied({ alpha: true }, "alpha")).toBe(false);
  expect(voiceReadinessDenied({}, "alpha")).toBe(false);
});
