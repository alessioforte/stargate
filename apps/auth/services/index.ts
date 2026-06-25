import AuthService from "./api";
import { Http } from "./http";

declare global {
  interface Window {
    __RUNTIME_CONFIG__?: Record<string, string>;
  }

  var __RUNTIME_CONFIG__: Record<string, string> | undefined;
}

class Service {
  public http: Http;
  public auth: AuthService;
  private readonly baseURL: string;

  constructor(baseURL: string) {
    this.baseURL = baseURL;
    this.http = new Http("", null, this.baseURL);
    this.auth = new AuthService(this.http, this.baseURL);
  }
}

function getRuntimeEnv(key: string): string {
  const candidates = [key, `VITE_${key}`, `PUBLIC_RUNTIME_${key}`];
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

let service: Service | null = null;

function initialize(): Service {
  const apiUrl = getRuntimeEnv("API_URL");
  return new Service(apiUrl);
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
