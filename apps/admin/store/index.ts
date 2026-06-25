import { create } from "zustand";
import type { StateCreator } from "zustand";
import { devtools } from "zustand/middleware";
import {
  clearTokens,
  getStoredTokens,
  logoutAndRedirect,
  redirectToHostedLogin,
} from "@/lib/oauth";
import services from "@/services";
import { getTokenExpiry } from "@/services/jwt";
import type { AdminHealth } from "@/services/types";
import type { Actions, AdminStatus, State } from "./types";

const initialState: State = {
  adminError: null,
  adminReturnPath: "/",
  adminSessionStatus: "checking",
  adminStatus: null,
  message: null,
  loading: false,
  theme: "system",
  language: "en",
};

function tokenIsValid(accessToken: string | undefined): accessToken is string {
  if (!accessToken) return false;

  const expiresAt = getTokenExpiry(accessToken);
  if (!expiresAt) return false;

  const nowSeconds = Math.floor(Date.now() / 1000);
  return expiresAt - nowSeconds > 30;
}

function mapAdminHealth(health: AdminHealth | null): AdminStatus | null {
  if (!health) return null;

  return {
    apiName: health.name,
    databaseStatus: health.database.status,
    redisStatus: health.redis.status,
    status: health.status,
    version: health.version,
  };
}

function errorMessage(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

export const store: StateCreator<State & Actions> = (set) => ({
  ...initialState,
  clearAdminError: () => set({ adminError: null }),
  ensureAdminSession: async (returnPath) => {
    set({
      adminError: null,
      adminReturnPath: returnPath,
      adminSessionStatus: "checking",
    });

    try {
      const tokens = getStoredTokens();
      const accessToken = tokens?.access_token;

      if (!tokenIsValid(accessToken)) {
        clearTokens();
        set({
          adminSessionStatus: "redirecting",
          adminStatus: null,
        });
        await redirectToHostedLogin(returnPath);
        return { status: "redirecting" };
      }

      services.setApiKey(accessToken);

      const response = await services.admin.getAdminHealth();

      if (response.error) {
        if (response.status === 401) {
          clearTokens();
          set({
            adminSessionStatus: "redirecting",
            adminStatus: null,
          });
          await redirectToHostedLogin(returnPath);
          return { status: "redirecting" };
        }

        const message = response.message ?? "Admin health check failed";
        set({
          adminError: message,
          adminReturnPath: returnPath,
          adminSessionStatus: "error",
          adminStatus: null,
        });
        return { status: "error", message };
      }

      set({
        adminError: null,
        adminSessionStatus: "ready",
        adminStatus: mapAdminHealth(response.data),
      });
      return { status: "ready" };
    } catch (error: unknown) {
      const message = errorMessage(error, "Unable to start admin auth");
      set({
        adminError: message,
        adminReturnPath: returnPath,
        adminSessionStatus: "error",
        adminStatus: null,
      });
      return { status: "error", message };
    }
  },
  logout: async (returnPath) => {
    set({
      adminError: null,
      adminReturnPath: returnPath,
      adminSessionStatus: "redirecting",
      adminStatus: null,
    });
    await logoutAndRedirect(returnPath);
  },
  signIn: async (returnPath) => {
    set({
      adminError: null,
      adminReturnPath: returnPath,
      adminSessionStatus: "redirecting",
      adminStatus: null,
    });
    await redirectToHostedLogin(returnPath);
  },
  setTheme: (theme: "light" | "dark" | "system") => set({ theme }),
  setLanguage: (lang: "en" | "it") => set({ language: lang }),
});

export default create(devtools(store));
