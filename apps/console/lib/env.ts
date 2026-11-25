import { appPath } from "@/lib/base-path";

export interface ConsoleAppConfig {
  /** Origin of the admin API (`/admin/*`). */
  apiUrl: string;
  /** OAuth/OIDC issuer: authorize, token, revoke, and logout. */
  issuerUrl: string;
  authUrl: string;
  oauthClientId: string;
  oauthRedirectUri: string;
}

function cleanValue(value: string | undefined) {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

function cleanUrl(value: string | undefined) {
  return cleanValue(value)?.replace(/\/+$/, "") ?? null;
}

function browserOrigin() {
  if (typeof window === "undefined") {
    throw new Error(
      "VITE_API_URL is required when window.location is unavailable",
    );
  }

  return window.location.origin;
}

function requiredEnv(key: string, value: string | undefined) {
  const cleaned = cleanValue(value);
  if (!cleaned) {
    throw new Error(`Missing env value: ${key}`);
  }

  return cleaned;
}

export function getConsoleAppConfig(): ConsoleAppConfig {
  const origin = browserOrigin();
  // Without an explicit admin API the Console calls its own origin; without
  // an explicit issuer OAuth stays there too, which is the single-origin
  // deployment both apps have always served.
  const apiUrl = cleanUrl(import.meta.env.VITE_API_URL) ?? origin;
  const issuerUrl = cleanUrl(import.meta.env.VITE_ISSUER_URL) ?? apiUrl;

  return {
    apiUrl,
    issuerUrl,
    authUrl: cleanUrl(import.meta.env.VITE_AUTH_URL) ?? `${issuerUrl}/auth`,
    oauthClientId: requiredEnv(
      "VITE_OAUTH_CLIENT_ID",
      import.meta.env.VITE_OAUTH_CLIENT_ID,
    ),
    oauthRedirectUri:
      cleanUrl(import.meta.env.VITE_OAUTH_REDIRECT_URI) ??
      `${origin}${appPath("/auth/callback")}`,
  };
}
