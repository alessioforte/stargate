import type { LoginMFAResponse, OtpChallengeResponse } from "@/services/types";
import { getAuthBasePath } from "@/lib/base-path";
import { getAuthAppConfig } from "@/lib/env";

const RETURN_TO_KEY = "stargate.auth.return_to";
const PENDING_MFA_KEY = "stargate.auth.pending_mfa";
const PENDING_PASSWORDLESS_KEY = "stargate.auth.pending_passwordless";

export interface PendingPasswordlessChallenge extends OtpChallengeResponse {
  email: string;
}

export interface RouterLike {
  replace: (href: string) => void;
}

interface SearchParamsLike {
  get: (name: string) => string | null;
}

function canUseSessionStorage() {
  return typeof window !== "undefined" && Boolean(window.sessionStorage);
}

function readJson<T>(key: string): T | null {
  if (!canUseSessionStorage()) return null;

  const raw = window.sessionStorage.getItem(key);
  if (!raw) return null;

  try {
    return JSON.parse(raw) as T;
  } catch {
    window.sessionStorage.removeItem(key);
    return null;
  }
}

function writeJson(key: string, value: unknown) {
  if (!canUseSessionStorage()) return;
  window.sessionStorage.setItem(key, JSON.stringify(value));
}

/**
 * Accept only in-app paths and absolute URLs on this page's origin or the
 * API's origin, so `return_to` cannot send users to another site after
 * sign-in (open redirect).
 */
export function safeReturnTo(value: string | null | undefined) {
  if (!value) return null;

  if (value.startsWith("/")) {
    return value.startsWith("//") || value.startsWith("/\\") ? null : value;
  }

  if (typeof window === "undefined") return null;

  try {
    const url = new URL(value);
    const allowedOrigins = [
      window.location.origin,
      new URL(getAuthAppConfig().apiUrl).origin,
    ];
    return allowedOrigins.includes(url.origin) ? url.toString() : null;
  } catch {
    return null;
  }
}

/**
 * `returnTo` for a server that redirects back here: in-app paths are relative
 * to this app's base path, so they become absolute URLs on this origin.
 */
export function absoluteReturnTo(returnTo: string | null) {
  if (!returnTo || /^https?:\/\//.test(returnTo)) return returnTo;
  const base = getAuthBasePath();
  return `${window.location.origin}${base === "/" ? "" : base}${returnTo}`;
}

export function getReturnTo(searchParams?: SearchParamsLike | null) {
  return safeReturnTo(searchParams?.get("return_to")) ?? getStoredReturnTo();
}

export function storeReturnTo(returnTo: string | null) {
  if (!canUseSessionStorage()) return;

  const safe = safeReturnTo(returnTo);
  if (safe) {
    window.sessionStorage.setItem(RETURN_TO_KEY, safe);
  }
}

export function getStoredReturnTo() {
  if (!canUseSessionStorage()) return null;
  return safeReturnTo(window.sessionStorage.getItem(RETURN_TO_KEY));
}

export function clearReturnTo() {
  if (!canUseSessionStorage()) return;
  window.sessionStorage.removeItem(RETURN_TO_KEY);
}

export function hrefWithReturnTo(path: string, returnTo: string | null) {
  if (!returnTo) return path;
  return `${path}?return_to=${encodeURIComponent(returnTo)}`;
}

export function redirectToReturnTo(
  router: RouterLike,
  returnTo: string | null,
) {
  const target = safeReturnTo(returnTo) ?? "/";

  if (/^https?:\/\//.test(target)) {
    window.location.assign(target);
    return;
  }

  router.replace(target);
}

export function storePendingMfa(challenge: LoginMFAResponse) {
  writeJson(PENDING_MFA_KEY, challenge);
}

export function getPendingMfa() {
  return readJson<LoginMFAResponse>(PENDING_MFA_KEY);
}

export function clearPendingMfa() {
  if (!canUseSessionStorage()) return;
  window.sessionStorage.removeItem(PENDING_MFA_KEY);
}

export function storePendingPasswordless(
  challenge: PendingPasswordlessChallenge,
) {
  writeJson(PENDING_PASSWORDLESS_KEY, challenge);
}

export function getPendingPasswordless() {
  return readJson<PendingPasswordlessChallenge>(PENDING_PASSWORDLESS_KEY);
}

export function clearPendingPasswordless() {
  if (!canUseSessionStorage()) return;
  window.sessionStorage.removeItem(PENDING_PASSWORDLESS_KEY);
}
