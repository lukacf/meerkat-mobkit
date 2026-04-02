import {
  normalizeIdentityStatusRow,
  normalizeResponsePhase,
  normalizeSidebarWatchFields,
} from "@console-core";
import type { ConsoleAgent, ConsoleExperience, ConsoleExperienceAgentSnapshotRow } from "../types";

export function normalizeAgents(
  experience: ConsoleExperience | null,
  modules: unknown[]
): ConsoleAgent[] {
  const identityStatusRows = Array.isArray(experience?.identity_status?.rows)
    ? experience.identity_status.rows
    : [];
  const normalizedIdentityStatusRows = identityStatusRows
    .map((entry) => normalizeIdentityStatusRow(entry))
    .filter((entry): entry is NonNullable<typeof entry> => entry !== null);
  const identityStatusByIdentity = new Map(
    normalizedIdentityStatusRows.map((row) => [row.identity, row] as const),
  );

  const snapshotAgents = experience?.agent_sidebar?.live_snapshot?.agents;
  if (Array.isArray(snapshotAgents) && snapshotAgents.length > 0) {
    return snapshotAgents.map((entry: ConsoleExperienceAgentSnapshotRow) => {
      const entryIdentity = typeof entry.identity === "string" ? entry.identity.trim() : "";
      const entryMemberId = typeof entry.member_id === "string" ? entry.member_id.trim() : "";
      const statusRow =
        identityStatusByIdentity.get(entryIdentity) ||
        identityStatusByIdentity.get(entryMemberId) ||
        normalizeIdentityStatusRow(entry);
      const watchFields = normalizeSidebarWatchFields(entry);
      const responsePhase = normalizeResponsePhase(entry.response_phase);

      return {
        ...(statusRow?.identity ? { identity: statusRow.identity } : entry.identity ? { identity: String(entry.identity) } : {}),
        agent_id: String(entry.agent_id || statusRow?.identity || entry.identity || entry.member_id || ""),
        member_id: String(entry.member_id || statusRow?.identity || entry.identity || entry.agent_id || ""),
        label: String(entry.label || statusRow?.display_name || entry.display_name || statusRow?.identity || entry.identity || entry.member_id || entry.agent_id || "unknown"),
        kind: String(entry.kind || statusRow?.profile || entry.profile || "module_agent"),
        ...(statusRow?.profile !== undefined
          ? { profile: statusRow.profile }
          : entry.profile !== undefined
            ? { profile: String(entry.profile) }
            : {}),
        ...(statusRow?.state !== undefined
          ? { state: statusRow.state }
          : entry.state !== undefined
            ? { state: String(entry.state) }
            : {}),
        ...(statusRow?.addressability ? { addressability: statusRow.addressability } : {}),
        ...(statusRow?.generation !== undefined ? { generation: statusRow.generation } : {}),
        ...(statusRow?.checkpoint_version !== undefined ? { checkpoint_version: statusRow.checkpoint_version } : {}),
        ...(statusRow?.lease_healthy !== undefined ? { lease_healthy: statusRow.lease_healthy } : {}),
        ...(responsePhase !== null && { response_phase: responsePhase }),
        ...(entry.wired_to !== undefined && { wired_to: entry.wired_to as string[] }),
        ...(statusRow?.labels && Object.keys(statusRow.labels).length > 0
          ? { labels: statusRow.labels }
          : entry.labels !== undefined
            ? { labels: entry.labels as Record<string, string> }
            : {}),
        ...(entry.group !== undefined && { group: String(entry.group) }),
        ...(entry.addressable !== undefined
          ? { addressable: Boolean(entry.addressable) }
          : statusRow?.addressability
            ? { addressable: statusRow.addressability === "addressable" }
            : {}),
        ...(entry.affordances !== undefined && { affordances: entry.affordances }),
        ...watchFields,
      };
    });
  }

  if (Array.isArray(identityStatusRows) && identityStatusRows.length > 0) {
    return identityStatusRows.map((entry) => {
      const statusRow = normalizeIdentityStatusRow(entry);
      const identity = statusRow?.identity || "";

      return {
        identity,
        agent_id: String(identity),
        member_id: identity ? `identity-only:${identity}` : "",
        label: String(statusRow?.display_name || identity || "unknown"),
        kind: String(statusRow?.profile || "identity"),
        ...(statusRow?.profile !== undefined ? { profile: statusRow.profile } : {}),
        ...(statusRow?.state !== undefined ? { state: statusRow.state } : {}),
        ...(statusRow?.addressability ? { addressability: statusRow.addressability } : {}),
        ...(statusRow?.generation !== undefined ? { generation: statusRow.generation } : {}),
        ...(statusRow?.checkpoint_version !== undefined ? { checkpoint_version: statusRow.checkpoint_version } : {}),
        ...(statusRow?.lease_healthy !== undefined ? { lease_healthy: statusRow.lease_healthy } : {}),
        ...(statusRow?.labels && Object.keys(statusRow.labels).length > 0 ? { labels: statusRow.labels } : {}),
        addressable: false,
        affordances: { can_send_message: false },
      };
    });
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
