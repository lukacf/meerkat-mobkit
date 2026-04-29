import type {
  ActivityFilterPreset,
  ConsoleDockTargetAddressingMode,
  ConsoleIdentityEventEnvelope,
  ConsoleInteractionAccepted,
  ConsoleInteractionRejectedError,
  ConsoleInteractionRequest,
  ExperienceSectionMeta,
  GatingActionRequest,
  GatingActionResult,
  IdentityStreamRequest,
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
  data: unknown;
}

export type ConsoleInteractRequest = ConsoleInteractionRequest;
export type ConsoleInteractAccepted = ConsoleInteractionAccepted;
export type ConsoleIdentityStreamSubscription = IdentityStreamRequest;
export type ConsoleIdentityStreamEvent = ConsoleIdentityEventEnvelope;

export interface ConsoleSendMessageResult {
  accepted?: boolean;
  member_id?: string;
  session_id?: string;
}

export type ConsoleGatewayInteractionRejectedError = ConsoleInteractionRejectedError;

export interface ConsoleAgentAffordances {
  addressable?: boolean;
  can_send_message?: boolean;
  can_retire?: boolean;
  can_respawn?: boolean;
  runtime_mode?: string;
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
  addressable?: boolean;
  affordances?: ConsoleAgentAffordances;
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
  addressable?: boolean;
  affordances?: ConsoleAgentAffordances;
}

export interface ConsoleTopologyNode {
  identity?: string;
  label?: string;
  role?: string;
  state?: string;
  wired_to?: string[];
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
}

export interface ConsoleModulesResponse {
  modules?: unknown[];
}

export interface ConsoleGatingActionPayload extends GatingActionRequest {}

export interface ConsoleGatingActionResponse extends GatingActionResult {}

export interface ConsoleReplayUnavailablePayload extends ReplayUnavailableError {}

export interface ConsoleSessionHistoryPage {
  session_id?: string;
  message_count?: number;
  offset?: number;
  limit?: number | null;
  has_more?: boolean;
  messages?: unknown[];
}

export interface ConsoleDockAddressedTarget {
  addressingMode: ConsoleDockTargetAddressingMode;
  identity?: string;
  memberId?: string;
}

export interface ConsoleToolAccumulatorSnapshot extends ToolCallAccumulatorState {}
