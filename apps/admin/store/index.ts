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
import type {
  AdminHealth,
  List,
  User,
  Query,
  JsonValue,
  CreateUserRequest,
  UpdateUserRequest,
} from "@/services/types";
import Service from "@/services";
import { showNotification } from "@/components";
import type { Actions, AdminStatus, State } from "./types";
import { StoreItem } from "./item";
import Settings from "./settings";

const initialState: State = {
  adminError: null,
  adminReturnPath: "/",
  adminSessionStatus: "checking",
  adminStatus: null,
  message: null,
  loading: false,
  theme: Settings.get("theme", "system"),
  language: Settings.get("language", "en"),

  users: new StoreItem<List<User>>(null),
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

export const store: StateCreator<State & Actions> = (set, get) => ({
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

  setTheme: (theme: "light" | "dark" | "system") => {
    set({ theme });
    Settings.set("theme", theme);
  },

  setLanguage: (lang: "en" | "it") => {
    set({ language: lang });
    Settings.set("language", lang);
  },

  getUsers: async (query?: Query) => {
    const users = get().users;
    set({ users: users.setLoading() });
    const { data, error, message } = await Service.admin.getUsers(query);
    if (error) {
      set({ users: users.setError(message) });
      return;
    }
    set({ users: users.setSuccess(data) });
  },

  createUser: async (user: CreateUserRequest) => {
    const users = get().users;
    set({ users: users.setLoading() });

    const { error, message } = await Service.admin.createUser(user);

    if (error) {
      set({ users: users.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to create user",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "User created successfully",
    });
    get().getUsers();
  },

  updateUser: async (id: string, user: UpdateUserRequest) => {
    const users = get().users;
    set({ users: users.setLoading() });

    const { error, message } = await Service.admin.updateUser(id, user);

    if (error) {
      set({ users: users.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to update user",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "User updated successfully",
    });

    get().getUsers();
  },

  updateUserAttrs: async (id: string, attrs: JsonValue) => {
    const users = get().users;
    set({ users: users.setLoading() });

    const { error, message } = await Service.admin.updateUserAttrs(id, {
      attrs,
    });

    if (error) {
      set({ users: users.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to update user attrs",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "User attrs updated successfully",
    });

    get().getUsers();
  },
});

export default create(devtools(store));
