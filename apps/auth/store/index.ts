import { create, type StateCreator } from "zustand";
import { devtools } from "zustand/middleware";
import {
  clearPendingMfa,
  clearPendingPasswordless,
  storePendingMfa,
  storePendingPasswordless,
} from "@/lib/auth-flow";
import services from "@/services";
import type { LoginMFAResponse, LoginResponse } from "@/services/types";
import type {
  State,
  Actions,
  AuthActionResult,
  ChallengeActionResult,
  MessageActionResult,
  OrganizationsLoadResult,
  SignupLoadResult,
} from "./types";
import {
  registerApiMessageCatalog,
  resolveApiMessage,
} from "@/i18n/api-messages";

const initialState: State = {
  accountOrganizations: null,
  message: null,
  loading: false,
  authStatus: "idle",
  pendingMfa: null,
  pendingPasswordless: null,
  signup: null,
  tokens: null,
  error: null,
  organizationError: null,
  organizationSwitchingId: null,
  language: "en",
};

function isLoginMFAResponse(
  data: LoginResponse | LoginMFAResponse | null,
): data is LoginMFAResponse {
  return Boolean(data && "mfaRequired" in data && data.mfaRequired);
}

function failure(message: string): AuthActionResult {
  return {
    status: "error",
    message,
  };
}

function challengeFailure(message: string): ChallengeActionResult {
  return {
    status: "error",
    message,
  };
}

function messageFailure(message: string): MessageActionResult {
  return {
    status: "error",
    message,
  };
}

function signupFailure(message: string): SignupLoadResult {
  return {
    status: "error",
    message,
  };
}

function organizationsFailure(message: string): OrganizationsLoadResult {
  return {
    status: "error",
    message,
  };
}

export const store: StateCreator<State & Actions> = (set, get) => ({
  ...initialState,
  setLanguage: (lang: "en" | "it") => {
    set({ language: lang });
  },
  loadApiMessages: async (locale) => {
    const response = await services.auth.getApiMessages(
      locale ?? get().language,
    );
    if (!response.error && response.data) {
      registerApiMessageCatalog(response.data);
    }
  },
  clearAuthError: () => set({ error: null, message: null }),
  login: async (credentials) => {
    set({
      loading: true,
      authStatus: "loading",
      message: null,
      error: null,
    });

    const response = await services.auth.login(credentials);

    if (response.error || !response.data) {
      const message = resolveApiMessage(
        get().language,
        "errors",
        response,
        "Login failed",
      );
      set({
        loading: false,
        authStatus: "error",
        error: message,
        message: { type: "error", text: message },
      });
      return failure(message);
    }

    if (response.status === 202 || isLoginMFAResponse(response.data)) {
      if (!isLoginMFAResponse(response.data)) {
        const message = "MFA challenge was not returned by the server";
        set({
          loading: false,
          authStatus: "error",
          error: message,
          message: { type: "error", text: message },
        });
        return failure(message);
      }

      set({
        loading: false,
        authStatus: "mfa_required",
        pendingMfa: response.data,
        error: null,
        message: {
          type: "info",
          text: "Additional verification is required to finish signing in.",
        },
      });
      storePendingMfa(response.data);
      return {
        status: "mfa_required",
        challenge: response.data,
      };
    }

    set({
      loading: false,
      authStatus: "authenticated",
      pendingMfa: null,
      pendingPasswordless: null,
      tokens: response.data,
      error: null,
      message: null,
    });
    clearPendingMfa();
    clearPendingPasswordless();
    return {
      status: "authenticated",
      tokens: response.data,
    };
  },
  verifyMFAChallenge: async (challengeId, code) => {
    set({
      loading: true,
      authStatus: "loading",
      message: null,
      error: null,
    });

    const response = await services.auth.verifyMFAChallenge(challengeId, code);

    if (response.error || !response.data) {
      const message = resolveApiMessage(
        get().language,
        "errors",
        response,
        "MFA verification failed",
      );
      set({
        loading: false,
        authStatus: "error",
        error: message,
        message: { type: "error", text: message },
      });
      return failure(message);
    }

    set({
      loading: false,
      authStatus: "authenticated",
      pendingMfa: null,
      pendingPasswordless: null,
      tokens: response.data,
      error: null,
      message: null,
    });
    clearPendingMfa();
    return {
      status: "authenticated",
      tokens: response.data,
    };
  },
  requestPasswordlessLogin: async (email) => {
    set({
      loading: true,
      authStatus: "loading",
      message: null,
      error: null,
    });

    const response = await services.auth.passwordlessLogin(email);

    if (response.error || !response.data) {
      const message = resolveApiMessage(
        get().language,
        "errors",
        response,
        "Passwordless login failed",
      );
      set({
        loading: false,
        authStatus: "error",
        error: message,
        message: { type: "error", text: message },
      });
      return challengeFailure(message);
    }

    const pendingPasswordless = {
      ...response.data,
      email,
    };

    set({
      loading: false,
      authStatus: "idle",
      pendingPasswordless,
      error: null,
      message: {
        type: "info",
        text: "A verification code was sent to your email.",
      },
    });
    storePendingPasswordless(pendingPasswordless);

    return {
      status: "challenge_sent",
      challenge: response.data,
    };
  },
  verifyPasswordlessLogin: async (challengeId, code) => {
    set({
      loading: true,
      authStatus: "loading",
      message: null,
      error: null,
    });

    const response = await services.auth.verifyPasswordlessLogin(
      challengeId,
      code,
    );

    if (response.error || !response.data) {
      const message = resolveApiMessage(
        get().language,
        "errors",
        response,
        "Passwordless verification failed",
      );
      set({
        loading: false,
        authStatus: "error",
        error: message,
        message: { type: "error", text: message },
      });
      return failure(message);
    }

    set({
      loading: false,
      authStatus: "authenticated",
      pendingMfa: null,
      pendingPasswordless: null,
      tokens: response.data,
      error: null,
      message: null,
    });
    clearPendingPasswordless();
    clearPendingMfa();

    return {
      status: "authenticated",
      tokens: response.data,
    };
  },
  forgotPassword: async (email) => {
    set({
      loading: true,
      message: null,
      error: null,
    });

    const response = await services.auth.forgotPassword(email);

    if (response.error || !response.data) {
      const message = resolveApiMessage(
        get().language,
        "errors",
        response,
        "Password reset request failed",
      );
      set({
        loading: false,
        error: message,
        message: { type: "error", text: message },
      });
      return messageFailure(message);
    }

    const message = resolveApiMessage(
      get().language,
      "messages",
      response.data,
      response.data.message,
    );
    set({
      loading: false,
      error: null,
      message: { type: "success", text: message },
    });

    return {
      status: "success",
      message,
    };
  },
  changePassword: async (password, token) => {
    set({
      loading: true,
      message: null,
      error: null,
    });

    const response = await services.auth.changePassword(password, token);

    if (response.error || !response.data) {
      const message = resolveApiMessage(
        get().language,
        "errors",
        response,
        "Password change failed",
      );
      set({
        loading: false,
        error: message,
        message: { type: "error", text: message },
      });
      return messageFailure(message);
    }

    const message = resolveApiMessage(
      get().language,
      "messages",
      response.data,
      response.data.message,
    );
    set({
      loading: false,
      error: null,
      message: { type: "success", text: message },
    });

    return {
      status: "success",
      message,
    };
  },
  loadSignup: async (token) => {
    set({
      loading: true,
      message: null,
      error: null,
      signup: null,
    });

    const response = await services.auth.getSignup(token);

    if (response.error || !response.data) {
      const message = resolveApiMessage(
        get().language,
        "errors",
        response,
        "Signup token verification failed",
      );
      set({
        loading: false,
        error: message,
        message: { type: "error", text: message },
        signup: null,
      });
      return signupFailure(message);
    }

    set({
      loading: false,
      error: null,
      message: null,
      signup: response.data,
    });

    return {
      status: "success",
      signup: response.data,
    };
  },
  completeSignup: async (payload) => {
    set({
      loading: true,
      message: null,
      error: null,
    });

    const response = await services.auth.completeSignup(payload);

    if (response.error || !response.data) {
      const message = resolveApiMessage(
        get().language,
        "errors",
        response,
        "Signup failed",
      );
      set({
        loading: false,
        error: message,
        message: { type: "error", text: message },
      });
      return messageFailure(message);
    }

    const message = resolveApiMessage(
      get().language,
      "messages",
      response.data,
      response.data.message,
    );
    set({
      loading: false,
      error: null,
      message: { type: "success", text: message },
    });

    return {
      status: "success",
      message,
    };
  },
  loadAccountOrganizations: async () => {
    set({
      accountOrganizations: null,
      organizationError: null,
    });

    const response = await services.auth.getOrganizations();
    if (response.error || !response.data) {
      const message = resolveApiMessage(
        get().language,
        "errors",
        response,
        "Failed to load organizations",
      );
      set({ organizationError: message });
      return organizationsFailure(message);
    }

    set({
      accountOrganizations: response.data.organizations,
      organizationError: null,
    });
    return {
      status: "success",
      organizations: response.data.organizations,
    };
  },
  switchOrganization: async (orgId) => {
    set({
      organizationError: null,
      organizationSwitchingId: orgId,
    });

    const response = await services.auth.switchOrganization(orgId);
    if (response.error) {
      const message = resolveApiMessage(
        get().language,
        "errors",
        response,
        "Failed to switch organization",
      );
      set({
        organizationError: message,
        organizationSwitchingId: null,
      });
      return messageFailure(message);
    }

    set({
      organizationError: null,
      organizationSwitchingId: null,
    });
    return {
      status: "success",
      message: response.message ?? "Organization selected",
    };
  },
});

export default create(devtools(store));
