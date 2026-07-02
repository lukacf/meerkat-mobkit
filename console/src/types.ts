import type {
  ActivityFilterPreset,
  ConsoleInteractionRejectedError,
  ExperienceSectionMeta,
  GatingActionRequest,
  GatingActionResult,
  IdentityStatusRow,
  IdentityInspectViewState,
  ReplayUnavailableError,
  ResponsePhase,
  RoutingSectionView,
  SidebarWatchFields,
  ToolCallAccumulatorState,
} from "@console-core";

export interface ConsoleFrame {
  id: string;
  event: string;
  identity?: string;
  interactionId?: string;
  timestampMs?: number;
  cursor?: string;
  runtimeKey?: string;
  sessionId?: string;
  status?: string;
  sourceKind?: string;
  frameVersion?: number;
  updatedAtMs?: number;
  turnId?: string;
  runId?: string;
  data: unknown;
}

export interface ConsoleTimelineAccepted {
  interaction_id: string;
  identity: string;
  conversation_id?: string;
  session_id?: string;
  input_frame_id?: string;
  cursor?: string;
  status?: string;
}

export interface ConsoleTimelinePage {
  frames: ConsoleFrame[];
  nextCursor?: string;
  latestCursor?: string;
  exhausted?: boolean;
  available: boolean;
}

export type ConsoleGatewayInteractionRejectedError = ConsoleInteractionRejectedError;

export interface ConsoleAgentAffordances {
  addressable?: boolean;
  can_send_message?: boolean;
  can_retire?: boolean;
  can_respawn?: boolean;
  runtime_mode?: string;
}

export interface ConsoleModelCapabilities {
  image_input?: boolean;
}

export interface ConsoleAgent extends SidebarWatchFields {
  identity?: string;
  agent_id: string;
  member_id: string;
  session_id?: string;
  label: string;
  kind: string;
  role?: string;
  state?: string;
  addressability?: IdentityStatusRow["addressability"];
  generation?: number;
  checkpoint_version?: number;
  lease_healthy?: boolean;
  response_phase?: ResponsePhase;
  wired_to?: string[];
  labels?: Record<string, string>;
  group?: string;
  subgroup?: string;
  addressable?: boolean;
  affordances?: ConsoleAgentAffordances;
  model_capabilities?: ConsoleModelCapabilities;
}

export interface ConsoleExperienceAgentSnapshotRow extends Partial<IdentityStatusRow>, SidebarWatchFields {
  identity?: string;
  agent_id?: string;
  member_id?: string;
  session_id?: string;
  label?: string;
  kind?: string;
  role?: string;
  state?: string;
  session_id?: string;
  response_phase?: ResponsePhase;
  wired_to?: string[];
  labels?: Record<string, string>;
  group?: string;
  subgroup?: string;
  addressable?: boolean;
  affordances?: ConsoleAgentAffordances;
  model_capabilities?: ConsoleModelCapabilities;
}

export interface ConsoleSidebarButtonConfig {
  id: string;
  label: string;
  control?: string;
  href?: string;
  target?: string;
  icon_name?: string;
  iconName?: string;
}

export interface ConsoleSidebarUiConfig {
  visible_controls?: string[];
  hidden_controls?: string[];
  buttons?: ConsoleSidebarButtonConfig[];
}

export interface ConsoleAgentListConfig {
  group_by?: string[];
  subgroup_by?: string[];
  section_order?: string[];
  fallback_group?: string;
  fallback_subgroup?: string;
  collapse_single_subgroup?: boolean;
  default_pinned_agent_ids?: string[];
  badges?: ConsoleAgentBadgeConfig[];
  sections?: ConsoleAgentSectionConfig[];
}

export interface ConsoleAgentBadgeConfig {
  id: string;
  label: string;
  field: string;
  tone?: string;
}

export interface ConsoleAgentSectionConfig {
  name: string;
  collapsed?: boolean;
  empty_title?: string;
  empty_text?: string;
}

export interface ConsoleBrandingConfig {
  label?: string;
  logo_url?: string;
  logo_alt?: string;
}

export interface ConsoleAppearanceConfig {
  default_theme?: string;
  default_variant?: string;
}

export interface ConsoleEnvironmentConfig {
  label?: string;
}

export interface ConsoleLayoutConfig {
  initial_preset?: string;
  initial_control?: string;
  initial_agent?: string;
  sidebar_collapsed?: boolean;
}

export interface ConsoleRailFilterPresetConfig {
  id: string;
  label: string;
  watchedOnly?: boolean;
  alertLevels?: string[];
}

export interface ConsoleRailUiConfig {
  visible?: boolean;
  collapsed?: boolean;
  active_preset_id?: string;
  empty_text?: string;
  filter_presets?: ConsoleRailFilterPresetConfig[];
}

export interface ConsoleActionsUiConfig {
  inspect_label?: string;
  chat_label?: string;
  send_label?: string;
  respawn_label?: string;
  retire_label?: string;
  reset_label?: string;
  show_inspect?: boolean;
  show_chat?: boolean;
  show_respawn?: boolean;
  show_retire?: boolean;
  show_reset?: boolean;
}

export interface ConsoleUiConfig {
  title?: string;
  brand?: ConsoleBrandingConfig;
  appearance?: ConsoleAppearanceConfig;
  environment?: ConsoleEnvironmentConfig;
  layout?: ConsoleLayoutConfig;
  rail?: ConsoleRailUiConfig;
  sidebar?: ConsoleSidebarUiConfig;
  agent_list?: ConsoleAgentListConfig;
  actions?: ConsoleActionsUiConfig;
}

export interface ConsolePolicyConfig {
  fetch_timeout_ms?: number;
  read_only?: boolean;
}

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

export interface ConsoleHealthSnapshot {
  loaded_modules?: unknown[];
  loaded_module_count?: number;
  running?: boolean;
  identities?: IdentityStatusRow[];
}

type ConsoleExperienceSection<T> = T & Partial<ExperienceSectionMeta>;

export interface ProfileCapabilities {
  instance_count?: number;
  addressable?: boolean;
  has_wiring?: boolean;
}

export interface RuntimeCapabilities {
  can_spawn_members?: boolean;
  can_send_messages?: boolean;
  can_wire_members?: boolean;
  can_retire_members?: boolean;
  available_spawn_modes?: string[];
  profile_capabilities?: Record<string, ProfileCapabilities>;
}

export interface ConsoleExperience {
  contract_version?: string;
  runtime_id?: string;
  console_config?: ConsoleUiConfig;
  console_policy?: ConsolePolicyConfig;
  runtime_capabilities?: RuntimeCapabilities;
  agent_sidebar?: ConsoleExperienceSection<{
    title?: string;
    live_snapshot?: {
      agents?: ConsoleExperienceAgentSnapshotRow[];
    };
  }>;
  activity_feed?: ConsoleExperienceSection<{
    title?: string;
    filter_presets?: ActivityFilterPreset[];
    active_preset_id?: string;
  }>;
  identity_status?: ConsoleExperienceSection<{
    title?: string;
    rows?: IdentityStatusRow[];
  }>;
  chat_inspector?: ConsoleExperienceSection<{
    title?: string;
    inspect_identity_method?: string;
    live_snapshot?: IdentityInspectViewState | null;
  }>;
  flows?: ConsoleExperienceSection<{
    title?: string;
    evaluate_method?: string;
    dispatch_method?: string;
  }>;
  session_history?: ConsoleExperienceSection<{
    title?: string;
    source_method?: string;
  }>;
  routing?: ConsoleExperienceSection<RoutingSectionView & {
    title?: string;
  }>;
  gating?: ConsoleExperienceSection<{
    title?: string;
    decide_method?: string;
    pending?: unknown[];
    audit?: unknown[];
  }>;
  topology?: ConsoleExperienceSection<{
    title?: string;
    live_snapshot?: {
      nodes?: ConsoleTopologyNode[];
      node_count?: number;
    };
  }>;
  health_overview?: ConsoleExperienceSection<{
    title?: string;
    live_snapshot?: ConsoleHealthSnapshot;
  }>;
  access?: ConsoleAccessSection;
  memory?: ConsoleMemorySection;
}

/// Per-caller memory-panel standing, projected by `/console/experience` when a
/// memory panel is wired on the runtime. `can_read` gates the Memory nav entry;
/// `can_review_quarantine` gates the quarantine tab.
export interface ConsoleMemorySection {
  available?: boolean;
  can_read?: boolean;
  can_review_quarantine?: boolean;
}

/// Per-caller access standing, projected by `/console/experience` when an
/// access controller is wired on the runtime.
export interface ConsoleAccessSection {
  available?: boolean;
  enabled?: boolean;
  subject?: string | null;
  groups?: string[];
  can_administer?: boolean;
}

export interface ConsoleAccessStatus extends ConsoleAccessSection {
  revision?: number;
  is_admin?: boolean;
  actions?: string[];
}

export interface ConsoleAccessRule {
  id: string;
  description?: string;
  effect?: "allow" | "deny";
  subjects?: string[];
  groups?: string[];
  actions: string[];
  agents?: string[];
  roles?: string[];
  match_labels?: Record<string, string>;
}

export interface ConsoleAccessGroup {
  description?: string;
  members?: string[];
}

export interface ConsoleAccessConfig {
  enabled?: boolean;
  admins?: string[];
  groups?: Record<string, ConsoleAccessGroup>;
  rules?: ConsoleAccessRule[];
}

// ── Memory panel (read-only) ──────────────────────────────────────────────
// Shapes mirror the server contract for the four `mobkit/memory/panel/*`
// JSON-RPC methods. All fields are best-effort/optional on the client so the
// panel degrades gracefully across runtime versions.

export type MemoryRecordScope =
  | { scope: "identity"; realm: string; identity: string }
  | { scope: "mob"; realm: string; mob: string }
  | { scope: "operator"; realm: string; operator: string }
  | { scope: "realm"; realm: string };

export type MemoryRecordKind =
  | "preference"
  | "fact"
  | "gotcha"
  | "procedure"
  | "relationship"
  | "open_loop"
  | "reference";

export type MemoryTrust =
  | "untrusted"
  | "agent_observed"
  | "agent_verified"
  | "application"
  | "operator";

export type MemoryRecordStatus =
  | { status: "active" }
  | { status: "superseded"; by?: string }
  | { status: "quarantined"; reason?: string }
  | { status: "tombstoned" };

export interface MemoryEvidenceRef {
  session_id?: string;
  generation?: number;
  revision?: string;
  range?: [number, number];
}

export type MemoryAuthor =
  | { author: "agent"; identity?: string }
  | { author: "steward"; run_id?: string }
  | { author: "distiller"; run_id?: string }
  | { author: "operator" }
  | { author: "application" };

export interface MemoryProvenance {
  evidence?: MemoryEvidenceRef[];
  author?: MemoryAuthor;
  verification?: {
    checked?: string;
    evidence?: MemoryEvidenceRef[];
  };
}

export interface MemoryUsage {
  injected_count?: number;
  last_injected_at_ms?: number;
  explicit_recall_count?: number;
  last_recalled_at_ms?: number;
  judged_useful_count?: number;
  last_useful_at_ms?: number;
}

export interface MemoryPanelRecord {
  id: string;
  scope: MemoryRecordScope;
  kind: MemoryRecordKind;
  title: string;
  description?: string;
  tags?: string[];
  provenance?: MemoryProvenance;
  trust: MemoryTrust;
  status: MemoryRecordStatus;
  supersedes?: string;
  derived_from?: string[];
  working_set_rank?: number;
  created_at_ms?: number;
  updated_at_ms?: number;
  usage?: MemoryUsage;
  body_bytes?: number;
}

export interface MemoryFullRecord extends Omit<MemoryPanelRecord, "body_bytes"> {
  body: string;
}

export interface MemoryInjectionEntry {
  record_id: string;
  identity: string;
  session_key?: string;
  surface: "build" | "turn";
  at_ms: number;
}

export interface MemoryPendingPromotion {
  realm: string;
  pending_id: string;
  record_id: string;
  scope_kind: string;
  scope_key: string;
  rationale?: string;
  status?: string;
  created_at_ms?: number;
}

export interface MemoryDreamRun {
  realm: string;
  run_id: string;
  first_op_at_ms?: number;
  last_op_at_ms?: number;
  ops?: number;
  op_kinds?: Record<string, number>;
  quarantined_ops?: number;
  memory_ids?: string[];
  rationales?: string[];
}

export interface MemoryPanelRecordsResult {
  records: MemoryPanelRecord[];
  next_cursor: string | null;
  realms: string[];
}

export interface MemoryPanelRecordResult {
  realm: string;
  record: MemoryFullRecord;
  chain: MemoryPanelRecord[];
  injections: MemoryInjectionEntry[];
}

export interface MemoryPanelQuarantineResult {
  records: MemoryPanelRecord[];
  pending_promotions: MemoryPendingPromotion[];
  realms: string[];
}

export interface MemoryPanelDreamsResult {
  runs: MemoryDreamRun[];
  realms: string[];
}

export interface ConsoleModulesResponse {
  modules?: unknown[];
}

export interface ConsoleGatingActionPayload extends GatingActionRequest {}

export interface ConsoleGatingActionResponse extends GatingActionResult {}

export interface ConsoleReplayUnavailablePayload extends ReplayUnavailableError {}

export interface ConsoleToolAccumulatorSnapshot extends ToolCallAccumulatorState {}
