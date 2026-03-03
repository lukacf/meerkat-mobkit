export interface ConsoleFrame {
  id: string;
  event: string;
  data: unknown;
}

export interface ConsoleAgent {
  agent_id: string;
  member_id: string;
  label: string;
  kind: string;
}

export interface ConsoleExperience {
  agent_sidebar?: {
    title?: string;
    live_snapshot?: {
      agents?: Array<{
        agent_id?: string;
        member_id?: string;
        label?: string;
        kind?: string;
      }>;
    };
  };
  activity_feed?: {
    title?: string;
  };
  chat_inspector?: {
    title?: string;
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
