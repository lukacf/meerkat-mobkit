// The shell provides React through window globals (react-globals.js); the
// classic JSX transform emits React.createElement calls that resolve to it.
import type * as ReactNS from "react";

declare global {
  const React: typeof ReactNS;
}

export {};
