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

/**
 * Native form target of the provider login, used when the login form is
 * submitted without JavaScript. Relative unless VITE_API_URL is set, so it
 * also resolves in the build-time prerender where there is no window.
 */
export function oauthLoginAction() {
  return `${cleanUrl(import.meta.env.VITE_API_URL) ?? ""}/oauth/login`;
}

/** Native form target of the logout confirmation page. */
export function oauthLogoutConfirmAction() {
  return `${cleanUrl(import.meta.env.VITE_API_URL) ?? ""}/oauth/logout/confirm`;
}

export function getAuthAppConfig(): AuthAppConfig {
  return {
    apiUrl: cleanUrl(import.meta.env.VITE_API_URL) ?? browserOrigin(),
  };
}
