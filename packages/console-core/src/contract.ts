export const CONSOLE_CONTRACT_VERSION = "0.5.0";

export const CONSOLE_REST_PATHS = {
  experience: "/console/experience",
  modules: "/console/modules",
  identities: "/console/identities",
  timeline: "/console/timeline",
  timelineStream: "/console/timeline/stream",
  identityTimelineStreamTemplate: "/console/identity/{identity}/stream",
  legacySend: "/console/send",
} as const;

export const CONSOLE_RPC_PATHS = {
  jsonRpc: "/console/rpc",
  multipartJsonRpc: "/console/rpc/multipart",
} as const;

export const CONSOLE_RPC_METHODS = {
  capabilities: "mobkit/capabilities",
  send: "mobkit/console/send",
  listIdentities: "mobkit/console/list_identities",
  inspectIdentity: "mobkit/console/inspect_identity",
  queryTimeline: "mobkit/console/query_timeline",
  blobUpload: "mobkit/blob/upload",
  retireIdentity: "mobkit/retire",
  respawnIdentity: "mobkit/respawn",
  resetIdentity: "mobkit/reset",
  routingRoutesList: "mobkit/routing/routes/list",
  deliveryHistory: "mobkit/delivery/history",
  gatingPending: "mobkit/gating/pending",
  gatingAudit: "mobkit/gating/audit",
  gatingDecide: "mobkit/gating/decide",
  accessStatus: "mobkit/access/status",
  accessGet: "mobkit/access/get",
  accessSet: "mobkit/access/set",
  accessEnable: "mobkit/access/enable",
  accessRuleUpsert: "mobkit/access/rules/upsert",
  accessRuleDelete: "mobkit/access/rules/delete",
  accessGroupSet: "mobkit/access/groups/set",
  accessGroupDelete: "mobkit/access/groups/delete",
  accessPreview: "mobkit/access/preview",
} as const;

export const CONSOLE_BLOB_PATH_PREFIX = "/blobs/";

export const CONSOLE_TIMELINE_QUERY_MODES = ["since", "recent"] as const;

export const CONSOLE_TIMELINE_REPLAY_UNAVAILABLE_CODE = -32013;
