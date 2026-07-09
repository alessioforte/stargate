export interface AuthAppConfig {
  apiUrl: string;
}

function cleanUrl(value: string | undefined) {
  const trimmed = value?.trim().replace(/\/+$/, "");
  return trimmed ? trimmed : null;
}

function browserOrigin() {
  if (typeof window === "undefined") {
    throw new Error(
      "VITE_API_URL is required when window.location is unavailable",
    );
  }

  return window.location.origin;
}

export function getAuthAppConfig(): AuthAppConfig {
  return {
    apiUrl: cleanUrl(import.meta.env.VITE_API_URL) ?? browserOrigin(),
  };
}
