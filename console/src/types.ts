export interface ConsoleFrame {
  id: string;
  event: string;
  data: unknown;
}

export interface ConsoleAgentAffordances {
  addressable?: boolean;
  can_send_message?: boolean;
  can_retire?: boolean;
  can_respawn?: boolean;
  runtime_mode?: string;
}

export interface ConsoleAgent {
  agent_id: string;
  member_id: string;
  label: string;
  kind: string;
  profile?: string;
  state?: string;
  wired_to?: string[];
  labels?: Record<string, string>;
  group?: string;
  addressable?: boolean;
  affordances?: ConsoleAgentAffordances;
}

export interface RuntimeCapabilities {
  can_spawn_members?: boolean;
  can_send_messages?: boolean;
  can_wire_members?: boolean;
  can_retire_members?: boolean;
  available_spawn_modes?: string[];
}

export interface ConsoleExperience {
  contract_version?: string;
  runtime_capabilities?: RuntimeCapabilities;
  agent_sidebar?: {
    title?: string;
    live_snapshot?: {
      agents?: Array<{
        agent_id?: string;
        member_id?: string;
        label?: string;
        kind?: string;
        profile?: string;
        state?: string;
        wired_to?: string[];
        labels?: Record<string, string>;
        group?: string;
        addressable?: boolean;
        affordances?: ConsoleAgentAffordances;
      }>;
    };
  };
  activity_feed?: {
    title?: string;
  };
  chat_inspector?: {
    title?: string;
  };
  flows?: {
    title?: string;
    list_method?: string;
    trigger_method?: string;
  };
  session_history?: {
    title?: string;
    source_method?: string;
  };
  topology?: {
    title?: string;
    live_snapshot?: {
      nodes?: unknown[];
      node_count?: number;
    };
  };
  health_overview?: {
    title?: string;
    live_snapshot?: {
      loaded_modules?: unknown[];
      loaded_module_count?: number;
      running?: boolean;
    };
  };
}

export interface ConsoleModulesResponse {
  modules?: unknown[];
}
