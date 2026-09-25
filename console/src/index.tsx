import React from "react";
import { createRoot } from "react-dom/client";
import { ConsoleApp } from "./ConsoleApp";
import { parseSseFrames } from "./lib/network";
import type { MarkdownUrlPolicy } from "@console-components";

interface CreateConsoleAppOptions {
  baseUrl?: string;
  /** Opaque host scope covering authority/runtime, realm and principal. */
  storageNamespace?: string;
  /** Host decisions for Markdown links and images. Default images stay disabled. */
  markdownUrlPolicy?: MarkdownUrlPolicy;
}

export function createConsoleApp(
  target: Element | DocumentFragment | null,
  options: CreateConsoleAppOptions = {},
): { unmount: () => void } {
  if (!target) {
    throw new Error("target element is required");
  }

  const baseUrl = options.baseUrl || "";
  const root = createRoot(target);
  root.render(<ConsoleApp baseUrl={baseUrl} storageNamespace={options.storageNamespace} markdownUrlPolicy={options.markdownUrlPolicy} />);

  return {
    unmount() {
      root.unmount();
    },
  };
}

export { ConsoleApp, parseSseFrames };
