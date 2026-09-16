import React from "react";
import { queryVoiceAvailability } from "./voice-session";

const NO_READINESS: Readonly<Record<string, boolean>> = {};

export function useVoiceReadiness(
  baseUrl: string,
  focusedIdentity: string | null,
  voiceIdentity: string | null,
  enabled: boolean,
): Readonly<Record<string, boolean>> {
  const key = JSON.stringify([baseUrl, focusedIdentity, voiceIdentity, enabled]);
  const [state, setState] = React.useState<{
    key: string;
    values: Readonly<Record<string, boolean>>;
  }>({ key: "", values: NO_READINESS });

  React.useEffect(() => {
    if (!enabled) return;
    const identities = [...new Set([focusedIdentity, voiceIdentity].filter(
      (identity): identity is string => Boolean(identity),
    ))];
    if (!identities.length) return;
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refresh = async () => {
      const entries = await Promise.all(identities.map(async (identity) =>
        [identity, await queryVoiceAvailability(baseUrl, identity)] as const,
      ));
      if (stopped) return;
      setState({ key, values: Object.fromEntries(entries) });
      timer = setTimeout(() => void refresh(), 15_000);
    };
    void refresh();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [baseUrl, enabled, focusedIdentity, key, voiceIdentity]);

  return enabled && state.key === key ? state.values : NO_READINESS;
}
