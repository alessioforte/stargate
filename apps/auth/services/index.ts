import AuthService from "./api";
import { Http } from "./http";
import { getAuthAppConfig } from "@/lib/env";

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

let service: Service | null = null;

function initialize(): Service {
  const apiUrl = getAuthAppConfig().apiUrl;
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
