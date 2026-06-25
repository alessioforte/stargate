import type { LoginMFAResponse, OtpChallengeResponse } from "@/services/types";

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

export function getReturnTo(searchParams?: SearchParamsLike | null) {
  return searchParams?.get("return_to") ?? getStoredReturnTo();
}

export function storeReturnTo(returnTo: string | null) {
  if (!canUseSessionStorage()) return;

  if (returnTo) {
    window.sessionStorage.setItem(RETURN_TO_KEY, returnTo);
  }
}

export function getStoredReturnTo() {
  if (!canUseSessionStorage()) return null;
  return window.sessionStorage.getItem(RETURN_TO_KEY);
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
  const target = returnTo ?? "/";

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
