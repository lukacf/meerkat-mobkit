import React from "react";
import { queryVoiceAvailability, type VoiceAvailability } from "./voice-session";

const NO_READINESS: Readonly<Record<string, boolean>> = {};
const READINESS_REFRESH_INTERVAL_MS = 15_000;

/**
 * Fold one poll round into the last known per-identity readiness. Definite gateway answers
 * replace the previous value; an `unknown` (the poll itself failed) keeps whatever was known
 * for that identity on the same gateway, or stays absent when nothing was ever confirmed.
 */
export function mergeVoiceReadiness(
  previous: Readonly<Record<string, boolean>>,
  results: ReadonlyArray<readonly [string, VoiceAvailability]>,
): Readonly<Record<string, boolean>> {
  const values: Record<string, boolean> = {};
  for (const [identity, availability] of results) {
    if (availability === "unknown") {
      if (identity in previous) values[identity] = previous[identity];
    } else {
      values[identity] = availability === "available";
    }
  }
  return values;
}

/** Voice must close only when the gateway definitely said no, never because a poll failed. */
export function voiceReadinessDenied(
  readiness: Readonly<Record<string, boolean>>,
  identity: string,
): boolean {
  return readiness[identity] === false;
}

export function useVoiceReadiness(
  baseUrl: string,
  focusedIdentity: string | null,
  voiceIdentity: string | null,
  enabled: boolean,
): Readonly<Record<string, boolean>> {
  const key = JSON.stringify([baseUrl, focusedIdentity, voiceIdentity, enabled]);
  const [state, setState] = React.useState<{
    key: string;
    baseUrl: string;
    values: Readonly<Record<string, boolean>>;
  }>({ key: "", baseUrl: "", values: NO_READINESS });

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
      setState((previous) => ({
        key,
        baseUrl,
        // Known values never transfer across gateways, even for the same identity.
        values: mergeVoiceReadiness(previous.baseUrl === baseUrl ? previous.values : NO_READINESS, entries),
      }));
      timer = setTimeout(() => void refresh(), READINESS_REFRESH_INTERVAL_MS);
    };
    void refresh();
    return () => {
      stopped = true;
      clearTimeout(timer);
    };
  }, [baseUrl, enabled, focusedIdentity, key, voiceIdentity]);

  return enabled && state.key === key ? state.values : NO_READINESS;
}
