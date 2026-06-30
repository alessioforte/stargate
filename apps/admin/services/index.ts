import { Http } from "./http";
import { TokenManager } from "./token-manager";
import type { TokenData } from "./token-manager";
import { parseJwt } from "./jwt";
import AdminApiService from "./api";
import {
  clearTokens,
  getStoredTokens,
  storeTokens,
  type OAuthTokenResponse,
} from "@/lib/oauth";

declare global {
  interface Window {
    __RUNTIME_CONFIG__?: Record<string, string>;
  }
  var __RUNTIME_CONFIG__: Record<string, string> | undefined;
}

export type AuthResponse = OAuthTokenResponse;

class Service {
  public http: Http;
  public tokenManager: TokenManager;
  private apiKey: string;
  private readonly baseURL: string;

  private apiKeyExpiration?: number;

  // api services
  public admin: AdminApiService;

  constructor(apiKey: string, baseURL: string) {
    this.apiKey = apiKey;
    this.baseURL = baseURL;
    this.http = new Http(this.apiKey, null, this.baseURL);

    this.tokenManager = new TokenManager({
      bufferSeconds: 60,
      onRefreshSuccess: (tokens: TokenData) => {
        this.setApiKey(tokens.access_token);
      },
      onRefreshFailure: () => {
        this.deleteTokens();
        if (typeof window !== "undefined") {
          window.location.href = "/";
        }
      },
    });

    // Wire the token manager into Http
    this.http.setTokenManager(this.tokenManager);

    this.admin = new AdminApiService(this.http);
  }

  public setApiKey(apiKey: string) {
    this.apiKey = apiKey;
    this.http.setApiKey(apiKey);

    const claims = parseJwt(apiKey);
    if (claims && typeof claims.exp === "number") {
      this.apiKeyExpiration = claims.exp;
    }
  }

  public verifyApiKeyExpiration() {
    if (this.apiKeyExpiration) {
      const currentTime = Math.floor(Date.now() / 1000);
      return this.apiKeyExpiration > currentTime;
    }
    return false;
  }

  storeTokens(auth: AuthResponse) {
    storeTokens(auth);
    this.setApiKey(auth.access_token);
  }

  getTokens() {
    const auth = getStoredTokens();
    if (!auth) {
      return { access_token: null, refresh_token: null };
    }
    const { access_token, refresh_token, id_token } = auth;
    return { access_token, refresh_token, id_token };
  }

  deleteTokens() {
    clearTokens();
  }
}

function getRuntimeEnv(key: string): string {
  const candidates = [key, `VITE_${key}`, key.replace(/^PUBLIC_RUNTIME_/, "")];
  const windowConfig =
    typeof window === "undefined" ? undefined : window.__RUNTIME_CONFIG__;

  for (const candidate of candidates) {
    const value =
      windowConfig?.[candidate] ??
      globalThis.__RUNTIME_CONFIG__?.[candidate] ??
      (import.meta.env as Record<string, string | undefined>)[candidate];

    if (value) {
      return value.replace(/\/+$/, "");
    }
  }

  throw new Error(`Missing runtime config value: ${key}`);
}

function getStoredAccessToken() {
  if (typeof window === "undefined") return "";
  return getStoredTokens()?.access_token ?? "";
}

let service: Service | null = null;

function initialize(): Service {
  const apiKey = getStoredAccessToken();
  const apiUrl = getRuntimeEnv("PUBLIC_RUNTIME_API_URL");

  return new Service(apiKey, apiUrl);
}

const proxy = new Proxy({} as Service, {
  get(_, prop, receiver) {
    if (!service) {
      service = initialize();
    }

    return Reflect.get(service, prop, receiver);
  },
});

export default proxy;
