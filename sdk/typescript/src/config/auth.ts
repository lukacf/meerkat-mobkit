/**
 * Auth configuration for MobKit runtime.
 */

// -- Google auth ----------------------------------------------------------

export interface GoogleAuthConfig {
  readonly clientId: string;
  readonly discoveryUrl: string;
  readonly audience: string | null;
  readonly leewaySeconds: number;
}

export function google(
  clientId: string,
  options?: {
    discoveryUrl?: string;
    audience?: string;
    leewaySeconds?: number;
  },
): GoogleAuthConfig {
  return {
    clientId,
    discoveryUrl:
      options?.discoveryUrl ??
      "https://accounts.google.com/.well-known/openid-configuration",
    audience: options?.audience ?? null,
    leewaySeconds: options?.leewaySeconds ?? 60,
  };
}

export function googleAuthConfigToDict(
  config: GoogleAuthConfig,
): Record<string, unknown> {
  return {
    provider: "google",
    client_id: config.clientId,
    discovery_url: config.discoveryUrl,
    audience: config.audience ?? config.clientId,
    leeway_seconds: config.leewaySeconds,
  };
}

// -- JWT auth -------------------------------------------------------------

export interface JwtAuthConfig {
  readonly sharedSecret: string;
  readonly issuer: string | null;
  readonly audience: string | null;
  readonly leewaySeconds: number;
}

export function jwt(
  sharedSecret: string,
  options?: {
    issuer?: string;
    audience?: string;
    leewaySeconds?: number;
  },
): JwtAuthConfig {
  return {
    sharedSecret,
    issuer: options?.issuer ?? null,
    audience: options?.audience ?? null,
    leewaySeconds: options?.leewaySeconds ?? 60,
  };
}

export function jwtAuthConfigToDict(
  config: JwtAuthConfig,
): Record<string, unknown> {
  return {
    provider: "jwt",
    shared_secret: config.sharedSecret,
    issuer: config.issuer,
    audience: config.audience,
    leeway_seconds: config.leewaySeconds,
  };
}
