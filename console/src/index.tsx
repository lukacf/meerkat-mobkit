import React from "react";
import { createRoot } from "react-dom/client";
import { ConsoleApp } from "./ConsoleApp";
import { parseSseFrames } from "./lib/network";

interface CreateConsoleAppOptions {
  baseUrl?: string;
}

export function createConsoleApp(
  target: Element | DocumentFragment | null,
  options: CreateConsoleAppOptions = {}
): { unmount: () => void } {
  if (!target) {
    throw new Error("target element is required");
  }

  const baseUrl = options.baseUrl || "";
  const root = createRoot(target);
  root.render(<ConsoleApp baseUrl={baseUrl} />);

  return {
    unmount() {
      root.unmount();
    },
  };
}

export { ConsoleApp, parseSseFrames };
