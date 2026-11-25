import { Http } from "./http";
import { TokenManager } from "./token-manager";
import AdminApiService from "./api";
import { clearTokens } from "@/lib/oauth";
import { appPath } from "@/lib/base-path";
import { getConsoleAppConfig } from "@/lib/env";

let admin: AdminApiService | undefined;

const services = {
  get admin(): AdminApiService {
    if (!admin) {
      const tokenManager = new TokenManager(() => {
        clearTokens();
        window.location.href = appPath("/");
      });

      const config = getConsoleAppConfig();
      admin = new AdminApiService(
        new Http(config.apiUrl, tokenManager),
        new Http(config.issuerUrl, tokenManager),
      );
    }

    return admin;
  },
};

export default services;
