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

/**
 * Starts signing in with a social provider (`google`, `github`). The server
 * redirects to the provider and, once signed in, to the organization picker
 * and then `returnTo` (an absolute URL, see `absoluteReturnTo`).
 */
export function socialSignInUrl(provider: string, returnTo: string | null) {
  const start = `${getAuthAppConfig().apiUrl}/oauth/${encodeURIComponent(provider)}/start`;
  return returnTo
    ? `${start}?return_to=${encodeURIComponent(returnTo)}`
    : start;
}

export function getAuthAppConfig(): AuthAppConfig {
  return {
    apiUrl: cleanUrl(import.meta.env.VITE_API_URL) ?? browserOrigin(),
  };
}
