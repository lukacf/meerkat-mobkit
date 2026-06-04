export type { ConsoleAgent, ConsoleFrame } from "@console-core/runtime-types";

export interface ConsoleTopologyNode {
  identity?: string;
  label?: string;
  role?: string;
  state?: string;
  wired_to?: string[];
  labels?: Record<string, string>;
  group?: string;
  subgroup?: string;
  addressable?: boolean;
}
