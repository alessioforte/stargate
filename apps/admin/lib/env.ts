import { appPath } from "@/lib/base-path";

export interface AdminAppConfig {
  apiUrl: string;
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

export function getAdminAppConfig(): AdminAppConfig {
  const origin = browserOrigin();

  return {
    apiUrl: cleanUrl(import.meta.env.VITE_API_URL) ?? origin,
    authUrl: cleanUrl(import.meta.env.VITE_AUTH_URL) ?? `${origin}/auth`,
    oauthClientId: requiredEnv(
      "VITE_OAUTH_CLIENT_ID",
      import.meta.env.VITE_OAUTH_CLIENT_ID,
    ),
    oauthRedirectUri:
      cleanUrl(import.meta.env.VITE_OAUTH_REDIRECT_URI) ??
      `${origin}${appPath("/auth/callback")}`,
  };
}
