import type { ConsoleAgent, ConsoleExperience } from "../types";

export function normalizeAgents(
  experience: ConsoleExperience | null,
  modules: unknown[]
): ConsoleAgent[] {
  const snapshotAgents = experience?.agent_sidebar?.live_snapshot?.agents;
  if (Array.isArray(snapshotAgents) && snapshotAgents.length > 0) {
    return snapshotAgents.map((entry) => ({
      agent_id: String(entry.agent_id || entry.member_id || ""),
      member_id: String(entry.member_id || entry.agent_id || ""),
      label: String(entry.label || entry.member_id || entry.agent_id || "unknown"),
      kind: String(entry.kind || "module_agent"),
      ...(entry.profile !== undefined && { profile: String(entry.profile) }),
      ...(entry.state !== undefined && { state: String(entry.state) }),
      ...(entry.wired_to !== undefined && { wired_to: entry.wired_to as string[] }),
      ...(entry.labels !== undefined && { labels: entry.labels as Record<string, string> }),
      ...(entry.addressable !== undefined && { addressable: Boolean(entry.addressable) }),
      ...(entry.affordances !== undefined && { affordances: entry.affordances }),
    }));
  }

  if (Array.isArray(modules) && modules.length > 0) {
    return modules.map((moduleId) => ({
      agent_id: String(moduleId),
      member_id: String(moduleId),
      label: String(moduleId),
      kind: "module_agent",
    }));
  }

  return [];
}
