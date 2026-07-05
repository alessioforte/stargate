import { getAdminAppConfig } from "@/lib/env";

export interface OAuthConfig {
  apiUrl: string;
  authUrl: string;
  clientId: string;
  redirectUri: string;
  scope: string;
}

export interface OAuthRequestState {
  codeVerifier: string;
  returnPath: string;
  state: string;
}

export interface OAuthTokenResponse {
  access_token: string;
  expires_in: number;
  id_token?: string;
  refresh_token?: string;
  scope?: string;
  token_type: string;
}

const OAUTH_REQUEST_KEY = "stargate.admin.oauth.request";
const AUTH_STORAGE_KEY = "auth";
const DEFAULT_SCOPE = "openid email profile offline_access";
const WEB_CRYPTO_UNAVAILABLE_MESSAGE =
  "Stargate Admin sign-in requires Web Crypto SHA-256 for OAuth PKCE. Open the admin UI over HTTPS, or use localhost when working on the same machine.";

function webCryptoUnavailableMessage() {
  const insecureContext =
    typeof window.isSecureContext === "boolean" && !window.isSecureContext;
  const detail = insecureContext
    ? "The current page is not a secure browser context."
    : "This browser does not expose crypto.subtle.digest.";

  return `${WEB_CRYPTO_UNAVAILABLE_MESSAGE} ${detail}`;
}

function getBrowserCrypto() {
  const browserCrypto = window.crypto;

  if (!browserCrypto?.getRandomValues || !browserCrypto.subtle?.digest) {
    throw new Error(webCryptoUnavailableMessage());
  }

  return browserCrypto;
}

function base64UrlEncode(bytes: ArrayBuffer) {
  const binary = String.fromCharCode(...new Uint8Array(bytes));
  return btoa(binary)
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/g, "");
}

function randomString(byteLength = 32) {
  const browserCrypto = getBrowserCrypto();
  const bytes = new Uint8Array(byteLength);
  browserCrypto.getRandomValues(bytes);
  return base64UrlEncode(bytes.buffer);
}

async function sha256(value: string) {
  const browserCrypto = getBrowserCrypto();
  const bytes = new TextEncoder().encode(value);
  return browserCrypto.subtle.digest("SHA-256", bytes);
}

export function getOAuthConfig(): OAuthConfig {
  const config = getAdminAppConfig();

  return {
    apiUrl: config.apiUrl,
    authUrl: config.authUrl,
    clientId: config.oauthClientId,
    redirectUri: config.oauthRedirectUri,
    scope: DEFAULT_SCOPE,
  };
}

export function getStoredTokens(): OAuthTokenResponse | null {
  try {
    const raw = window.localStorage.getItem(AUTH_STORAGE_KEY);
    return raw ? (JSON.parse(raw) as OAuthTokenResponse) : null;
  } catch {
    return null;
  }
}

export function storeTokens(tokens: OAuthTokenResponse) {
  window.localStorage.setItem(AUTH_STORAGE_KEY, JSON.stringify(tokens));
}

export function clearTokens() {
  window.localStorage.removeItem(AUTH_STORAGE_KEY);
}

export function storeOAuthRequest(request: OAuthRequestState) {
  window.sessionStorage.setItem(OAUTH_REQUEST_KEY, JSON.stringify(request));
}

export function getOAuthRequest(): OAuthRequestState | null {
  try {
    const raw = window.sessionStorage.getItem(OAUTH_REQUEST_KEY);
    return raw ? (JSON.parse(raw) as OAuthRequestState) : null;
  } catch {
    window.sessionStorage.removeItem(OAUTH_REQUEST_KEY);
    return null;
  }
}

export function clearOAuthRequest() {
  window.sessionStorage.removeItem(OAUTH_REQUEST_KEY);
}

export async function buildAuthorizeUrl(returnPath: string) {
  const config = getOAuthConfig();
  const codeVerifier = randomString(64);
  const codeChallenge = base64UrlEncode(await sha256(codeVerifier));
  const state = randomString(32);

  storeOAuthRequest({
    codeVerifier,
    returnPath,
    state,
  });

  const params = new URLSearchParams({
    response_type: "code",
    client_id: config.clientId,
    redirect_uri: config.redirectUri,
    scope: config.scope,
    state,
    code_challenge: codeChallenge,
    code_challenge_method: "S256",
  });

  return `${config.apiUrl}/oauth/authorize?${params.toString()}`;
}

function authAppUrl(authUrl: string, path: string) {
  const base = authUrl.endsWith("/") ? authUrl : `${authUrl}/`;
  return new URL(path.replace(/^\/+/, ""), base);
}

export async function redirectToHostedLogin(returnPath: string) {
  const config = getOAuthConfig();
  const authorizeUrl = await buildAuthorizeUrl(returnPath);
  const loginUrl = authAppUrl(config.authUrl, "/login");
  loginUrl.searchParams.set("return_to", authorizeUrl);
  window.location.assign(loginUrl.toString());
}

async function postTokenRequest(
  config: OAuthConfig,
  body: URLSearchParams,
): Promise<OAuthTokenResponse> {
  const response = await fetch(`${config.apiUrl}/oauth/token`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-www-form-urlencoded",
    },
    body,
  });

  if (!response.ok) {
    const error = (await response.json().catch(() => null)) as {
      error?: string;
      error_description?: string;
      message?: string;
    } | null;
    throw new Error(
      error?.error_description ??
        error?.message ??
        error?.error ??
        "OAuth token request failed",
    );
  }

  return response.json() as Promise<OAuthTokenResponse>;
}

export function exchangeAuthorizationCode(code: string, codeVerifier: string) {
  const config = getOAuthConfig();
  return postTokenRequest(
    config,
    new URLSearchParams({
      grant_type: "authorization_code",
      client_id: config.clientId,
      redirect_uri: config.redirectUri,
      code,
      code_verifier: codeVerifier,
    }),
  );
}

export function refreshAccessToken(refreshToken: string) {
  const config = getOAuthConfig();
  return postTokenRequest(
    config,
    new URLSearchParams({
      grant_type: "refresh_token",
      client_id: config.clientId,
      refresh_token: refreshToken,
    }),
  );
}

export async function revokeToken(token: string, tokenTypeHint?: string) {
  const config = getOAuthConfig();
  const body = new URLSearchParams({
    token,
    client_id: config.clientId,
  });

  if (tokenTypeHint) {
    body.set("token_type_hint", tokenTypeHint);
  }

  const response = await fetch(`${config.apiUrl}/oauth/revoke`, {
    method: "POST",
    headers: {
      "Content-Type": "application/x-www-form-urlencoded",
    },
    body,
  });

  if (!response.ok) {
    const error = (await response.json().catch(() => null)) as {
      error?: string;
      error_description?: string;
      message?: string;
    } | null;
    throw new Error(
      error?.error_description ??
        error?.message ??
        error?.error ??
        "OAuth revoke request failed",
    );
  }
}

export async function logoutAndRedirect(returnPath = "/") {
  const tokens = getStoredTokens();

  if (tokens?.refresh_token) {
    await revokeToken(tokens.refresh_token, "refresh_token").catch(() => null);
  }

  clearTokens();
  clearOAuthRequest();
  await redirectToHostedLogin(returnPath);
}
