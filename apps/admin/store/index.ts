import { create } from "zustand";
import type { StateCreator } from "zustand";
import { devtools } from "zustand/middleware";
import {
  clearOAuthRequest,
  clearTokens,
  exchangeAuthorizationCode,
  getOAuthRequest,
  getStoredTokens,
  logoutAndRedirect,
  redirectToHostedLogin,
} from "@/lib/oauth";
import services from "@/services";
import type { Response as ApiResponse } from "@/services/http";
import { getTokenExpiry } from "@/services/jwt";
import type {
  AccessControlRulesResponse,
  AdminKey,
  AdminHealth,
  ApiKey,
  ApiKeyQuery,
  Configuration,
  CreateAdminKeyRequest,
  CreateAdminKeyResponse,
  CreateApiKeyRequest,
  CreateApiKeyResponse,
  CreateOAuthClientRequest,
  CreateOAuthClientResponse,
  List,
  MessageResponse,
  OAuthClient,
  Organization,
  OutboxEvent,
  ServiceAccount,
  User,
  Query,
  JsonValue,
  CreateOrganizationRequest,
  CreateServiceAccountRequest,
  CreateUserRequest,
  CreateUserInvitationRequest,
  UpdateAdminKeyPermissionsRequest,
  RotateOAuthClientSecretResponse,
  UpdateOAuthClientRequest,
  UpdateOrganizationRequest,
  UpdateServiceAccountRequest,
  UpdateUserRequest,
  UpdateAccessControlRulesRequest,
  UserOrganization,
  ValidateAccessControlRulesRequest,
  ValidateAccessControlRulesResponse,
  EvaluateAccessControlRequest,
  EvaluateAccessControlResponse,
} from "@/services/types";
import Service from "@/services";
import { showNotification } from "@/components";
import type {
  Actions,
  AdminStatus,
  OAuthCallbackParams,
  OAuthCallbackResult,
  SelectOption,
  State,
} from "./types";
import { StoreItem } from "./item";
import Settings from "./settings";
import {
  registerApiMessageCatalog,
  resolveApiMessage,
} from "@/i18n/api-messages";
import { translate } from "@/i18n";

const initialState: State = {
  adminError: null,
  adminReturnPath: "/",
  adminSessionStatus: "checking",
  adminStatus: null,
  message: null,
  loading: false,
  language: Settings.get("language", "en"),

  accessControlRules: new StoreItem<AccessControlRulesResponse>(null),
  accessControlValidation: new StoreItem<ValidateAccessControlRulesResponse>(
    null,
  ),
  accessControlEvaluation: new StoreItem<EvaluateAccessControlResponse>(null),
  adminKeys: new StoreItem<List<AdminKey>>(null),
  apiKeys: new StoreItem<List<ApiKey>>(null),
  configuration: new StoreItem<Configuration>(null),
  oauthClients: new StoreItem<List<OAuthClient>>(null),
  organizations: new StoreItem<List<Organization>>(null),
  serviceAccounts: new StoreItem<List<ServiceAccount>>(null),
  users: new StoreItem<List<User>>(null),
  usersQuery: {},
  userOrganizations: new StoreItem<UserOrganization[], string>(null, null),
  superAdminUsers: new StoreItem<List<User>>(null),
  outboxEvents: new StoreItem<List<OutboxEvent>>(null),
  outboxEventsQuery: {},
  selectedOutboxEvent: new StoreItem<OutboxEvent>(null),
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

  const storeStatus =
    health.redis?.status ?? (health.store ? "memory" : "unknown");

  return {
    apiName: health.name,
    databaseStatus: health.database?.status ?? "unknown",
    status: health.status,
    storeStatus,
    version: health.version,
  };
}

function accessControlValidationFromRules(
  response: AccessControlRulesResponse,
): ValidateAccessControlRulesResponse {
  return {
    diagnostics: response.diagnostics,
    rules: response.rules,
    valid: response.valid,
  };
}

function errorMessage(error: unknown, fallback: string) {
  return error instanceof Error ? error.message : fallback;
}

function notificationText(
  locale: string,
  key: string,
  values?: Record<string, string | number>,
) {
  return translate(locale, `notifications.${key}`, values);
}

function showStoreNotification(
  locale: string,
  type: "error" | "success",
  message: string,
) {
  showNotification({
    type,
    title: translate(locale, type),
    message,
  });
}

async function localizedApiResponse<T>(
  locale: string,
  request: Promise<ApiResponse<T>>,
): Promise<ApiResponse<T>> {
  const response = await request;
  if (!response.code) {
    const data = response.data;
    if (
      data &&
      typeof data === "object" &&
      "code" in data &&
      "message" in data &&
      typeof data.code === "string" &&
      typeof data.message === "string"
    ) {
      const apiMessage = data as MessageResponse;
      return {
        ...response,
        data: {
          ...apiMessage,
          message: resolveApiMessage(
            locale,
            "messages",
            apiMessage,
            apiMessage.message,
          ),
        } as T,
      };
    }

    return response;
  }

  return {
    ...response,
    message: resolveApiMessage(
      locale,
      response.error ? "errors" : "messages",
      response,
      response.message ?? notificationText(locale, "requestFailed"),
    ),
  };
}

export const store: StateCreator<State & Actions> = (set, get) => ({
  ...initialState,

  clearAdminError: () => set({ adminError: null }),

  completeOAuthCallback: async (
    params: OAuthCallbackParams,
  ): Promise<OAuthCallbackResult> => {
    const request = getOAuthRequest();

    if (!request || !params.state || request.state !== params.state) {
      clearTokens();
      clearOAuthRequest();
      return {
        status: "error",
        message: notificationText(get().language, "invalidOAuthState"),
      };
    }

    if (params.error) {
      if (params.error === "login_required") {
        try {
          await redirectToHostedLogin(request.returnPath);
          return { status: "redirecting" };
        } catch (error: unknown) {
          return {
            status: "error",
            message: errorMessage(
              error,
              notificationText(get().language, "unableToRestartSignIn"),
            ),
          };
        }
      }

      clearOAuthRequest();
      return {
        status: "error",
        message:
          params.errorDescription ??
          notificationText(get().language, "oauthAuthorizationFailed", {
            error: params.error,
          }),
      };
    }

    if (!params.code) {
      return {
        status: "error",
        message: notificationText(
          get().language,
          "missingOAuthAuthorizationCode",
        ),
      };
    }

    try {
      const tokens = await exchangeAuthorizationCode(
        params.code,
        request.codeVerifier,
      );
      services.storeTokens(tokens);
      clearOAuthRequest();
      return {
        status: "success",
        returnPath: request.returnPath || "/",
      };
    } catch (error: unknown) {
      clearTokens();
      return {
        status: "error",
        message: errorMessage(
          error,
          notificationText(get().language, "oauthCallbackFailed"),
        ),
      };
    }
  },

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

      const response = await localizedApiResponse(
        get().language,
        services.admin.getAdminHealth(),
      );

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

        const message =
          response.message ??
          notificationText(get().language, "adminHealthCheckFailed");
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
      const message = errorMessage(
        error,
        notificationText(get().language, "unableToStartAdminAuth"),
      );
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
    try {
      await logoutAndRedirect(returnPath);
    } catch (error: unknown) {
      const message = errorMessage(
        error,
        notificationText(get().language, "unableToRestartAdminAuth"),
      );
      set({
        adminError: message,
        adminReturnPath: returnPath,
        adminSessionStatus: "error",
        adminStatus: null,
      });
    }
  },

  signIn: async (returnPath) => {
    set({
      adminError: null,
      adminReturnPath: returnPath,
      adminSessionStatus: "redirecting",
      adminStatus: null,
    });
    try {
      await redirectToHostedLogin(returnPath);
      return { status: "redirecting" };
    } catch (error: unknown) {
      const message = errorMessage(
        error,
        notificationText(get().language, "unableToStartAdminAuth"),
      );
      set({
        adminError: message,
        adminReturnPath: returnPath,
        adminSessionStatus: "error",
        adminStatus: null,
      });
      return { status: "error", message };
    }
  },

  setLanguage: (lang: "en" | "it") => {
    set({ language: lang });
    Settings.set("language", lang);
  },

  loadApiMessages: async (locale) => {
    const response = await services.admin.getApiMessages(
      locale ?? get().language,
    );
    if (!response.error && response.data) {
      registerApiMessageCatalog(response.data);
    }
  },

  getAdminKeys: async (query?: Query) => {
    const adminKeys = get().adminKeys;
    set({ adminKeys: adminKeys.setLoading() });
    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.getAdminKeys(query),
    );
    if (error) {
      set({ adminKeys: adminKeys.setError(message) });
      return;
    }
    set({ adminKeys: adminKeys.setSuccess(data) });
  },

  createAdminKey: async (
    adminKey: CreateAdminKeyRequest,
  ): Promise<CreateAdminKeyResponse | null> => {
    const adminKeys = get().adminKeys;
    set({ adminKeys: adminKeys.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.createAdminKey(adminKey),
    );

    if (error) {
      set({ adminKeys: adminKeys.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "adminKeyCreateFailed"),
      );
      return null;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "adminKeyCreated"),
    );
    get().getAdminKeys();
    return data;
  },

  updateAdminKeyPermissions: async (
    id: string,
    adminKey: UpdateAdminKeyPermissionsRequest,
  ) => {
    const adminKeys = get().adminKeys;
    set({ adminKeys: adminKeys.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.updateAdminKeyPermissions(id, adminKey),
    );

    if (error) {
      set({ adminKeys: adminKeys.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ??
          notificationText(get().language, "adminKeyPermissionsUpdateFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "adminKeyPermissionsUpdated"),
    );

    get().getAdminKeys();
  },

  revokeAdminKey: async (id: string) => {
    const adminKeys = get().adminKeys;
    set({ adminKeys: adminKeys.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.revokeAdminKey(id),
    );

    if (error) {
      set({ adminKeys: adminKeys.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "adminKeyRevokeFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      data?.message ??
        message ??
        notificationText(get().language, "adminKeyRevoked"),
    );

    get().getAdminKeys();
  },

  getApiKeys: async (query?: ApiKeyQuery) => {
    const apiKeys = get().apiKeys;
    set({ apiKeys: apiKeys.setLoading() });
    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.getApiKeys(query),
    );
    if (error) {
      set({ apiKeys: apiKeys.setError(message) });
      return;
    }
    set({ apiKeys: apiKeys.setSuccess(data) });
  },

  createApiKey: async (
    apiKey: CreateApiKeyRequest,
  ): Promise<CreateApiKeyResponse | null> => {
    const apiKeys = get().apiKeys;
    set({ apiKeys: apiKeys.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.createApiKey(apiKey),
    );

    if (error) {
      set({ apiKeys: apiKeys.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "apiKeyCreateFailed"),
      );
      return null;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "apiKeyCreated"),
    );
    get().getApiKeys();
    return data;
  },

  updateApiKeyAttrs: async (id: string, attrs: JsonValue) => {
    const apiKeys = get().apiKeys;
    set({ apiKeys: apiKeys.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.updateApiKeyAttrs(id, { attrs }),
    );

    if (error) {
      set({ apiKeys: apiKeys.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "apiKeyAttrsUpdateFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "apiKeyAttrsUpdated"),
    );

    get().getApiKeys();
  },

  revokeApiKey: async (id: string) => {
    const apiKeys = get().apiKeys;
    set({ apiKeys: apiKeys.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.revokeApiKey(id),
    );

    if (error) {
      set({ apiKeys: apiKeys.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "apiKeyRevokeFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      data?.message ??
        message ??
        notificationText(get().language, "apiKeyRevoked"),
    );

    get().getApiKeys();
  },

  deleteApiKey: async (id: string) => {
    const apiKeys = get().apiKeys;
    set({ apiKeys: apiKeys.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.deleteApiKey(id),
    );

    if (error) {
      set({ apiKeys: apiKeys.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "apiKeyDeleteFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      data?.message ??
        message ??
        notificationText(get().language, "apiKeyDeleted"),
    );

    get().getApiKeys();
  },

  getConfigurations: async () => {
    const configuration = get().configuration;
    set({ configuration: configuration.setLoading() });
    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.getConfigurations(),
    );
    if (error) {
      set({ configuration: configuration.setError(message) });
      return;
    }
    set({ configuration: configuration.setSuccess(data) });
  },

  updateConfigurations: async (configuration: Configuration) => {
    const configurationItem = get().configuration;
    set({ configuration: configurationItem.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.updateConfigurations(configuration),
    );

    if (error) {
      set({ configuration: configurationItem.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ??
          notificationText(get().language, "gatewayConfigurationUpdateFailed"),
      );
      return false;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "gatewayConfigurationUpdated"),
    );

    set({ configuration: configurationItem.setSuccess(configuration) });
    return true;
  },

  getAccessControlRules: async () => {
    const accessControlRules = get().accessControlRules;
    set({ accessControlRules: accessControlRules.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.getAccessControlRules(),
    );
    if (error) {
      set({ accessControlRules: accessControlRules.setError(message) });
      return;
    }

    set({
      accessControlRules: accessControlRules.setSuccess(data),
      accessControlValidation: get().accessControlValidation.setSuccess(
        data ? accessControlValidationFromRules(data) : null,
      ),
    });
  },

  updateAccessControlRules: async (
    request: UpdateAccessControlRulesRequest,
  ) => {
    const accessControlRules = get().accessControlRules;
    set({ accessControlRules: accessControlRules.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.updateAccessControlRules(request),
    );

    if (error) {
      set({ accessControlRules: accessControlRules.setError(message) });
      showStoreNotification(
        get().language,
        "error",
        message ?? translate(get().language, "failedToSaveAccessControlRules"),
      );
      return null;
    }

    set({
      accessControlRules: accessControlRules.setSuccess(data),
      accessControlValidation: get().accessControlValidation.setSuccess(
        data ? accessControlValidationFromRules(data) : null,
      ),
    });

    showStoreNotification(
      get().language,
      "success",
      translate(get().language, "rulesSaved"),
    );

    return data;
  },

  validateAccessControlRules: async (
    request: ValidateAccessControlRulesRequest,
  ) => {
    const accessControlValidation = get().accessControlValidation;
    set({
      accessControlValidation: accessControlValidation.setLoading(),
    });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.validateAccessControlRules(request),
    );

    if (error) {
      set({
        accessControlValidation: accessControlValidation.setError(message),
      });
      return null;
    }

    set({
      accessControlValidation: accessControlValidation.setSuccess(data),
    });
    return data;
  },

  evaluateAccessControlRules: async (request: EvaluateAccessControlRequest) => {
    const accessControlEvaluation = get().accessControlEvaluation;
    set({
      accessControlEvaluation: accessControlEvaluation.setLoading(),
    });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.evaluateAccessControlRules(request),
    );

    if (error) {
      set({
        accessControlEvaluation: accessControlEvaluation.setError(message),
      });
      return null;
    }

    set({
      accessControlEvaluation: accessControlEvaluation.setSuccess(data),
    });
    return data;
  },

  getOAuthClients: async (query?: Query) => {
    const oauthClients = get().oauthClients;
    set({ oauthClients: oauthClients.setLoading() });
    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.getOAuthClients(query),
    );
    if (error) {
      set({ oauthClients: oauthClients.setError(message) });
      return;
    }
    set({ oauthClients: oauthClients.setSuccess(data) });
  },

  createOAuthClient: async (
    oauthClient: CreateOAuthClientRequest,
  ): Promise<CreateOAuthClientResponse | null> => {
    const oauthClients = get().oauthClients;
    set({ oauthClients: oauthClients.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.createOAuthClient(oauthClient),
    );

    if (error) {
      set({ oauthClients: oauthClients.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "oauthClientCreateFailed"),
      );
      return null;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "oauthClientCreated"),
    );
    get().getOAuthClients();
    return data;
  },

  updateOAuthClient: async (
    clientId: string,
    oauthClient: UpdateOAuthClientRequest,
  ) => {
    const oauthClients = get().oauthClients;
    set({ oauthClients: oauthClients.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.updateOAuthClient(clientId, oauthClient),
    );

    if (error) {
      set({ oauthClients: oauthClients.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "oauthClientUpdateFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "oauthClientUpdated"),
    );

    get().getOAuthClients();
  },

  enableOAuthClient: async (clientId: string) => {
    const oauthClients = get().oauthClients;
    set({ oauthClients: oauthClients.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.enableOAuthClient(clientId),
    );

    if (error) {
      set({ oauthClients: oauthClients.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "oauthClientEnableFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "oauthClientEnabled"),
    );

    get().getOAuthClients();
  },

  disableOAuthClient: async (clientId: string) => {
    const oauthClients = get().oauthClients;
    set({ oauthClients: oauthClients.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.disableOAuthClient(clientId),
    );

    if (error) {
      set({ oauthClients: oauthClients.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "oauthClientDisableFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "oauthClientDisabled"),
    );

    get().getOAuthClients();
  },

  deleteOAuthClient: async (clientId: string) => {
    const oauthClients = get().oauthClients;
    set({ oauthClients: oauthClients.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.deleteOAuthClient(clientId),
    );

    if (error) {
      set({ oauthClients: oauthClients.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "oauthClientDeleteFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      data?.message ??
        message ??
        notificationText(get().language, "oauthClientDeleted"),
    );

    get().getOAuthClients();
  },

  rotateOAuthClientSecret: async (
    clientId: string,
  ): Promise<RotateOAuthClientSecretResponse | null> => {
    const oauthClients = get().oauthClients;
    set({ oauthClients: oauthClients.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.rotateOAuthClientSecret(clientId),
    );

    if (error) {
      set({ oauthClients: oauthClients.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ??
          notificationText(get().language, "oauthClientSecretRotateFailed"),
      );
      return null;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "oauthClientSecretRotated"),
    );

    get().getOAuthClients();
    return data;
  },

  getOrganizations: async (query?: Query) => {
    const organizations = get().organizations;
    set({ organizations: organizations.setLoading() });
    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.getOrganizations(query),
    );
    if (error) {
      set({ organizations: organizations.setError(message) });
      return;
    }
    set({ organizations: organizations.setSuccess(data) });
  },

  searchOrganizationOptions: async (
    query: string,
    excludedIds: string[] = [],
  ): Promise<SelectOption[]> => {
    const { data, error } = await localizedApiResponse(
      get().language,
      Service.admin.getOrganizations({
        q: query || undefined,
        limit: 20,
      }),
    );
    if (error) return [];

    const excluded = new Set(excludedIds);
    return (data?.data ?? [])
      .filter((organization) => !excluded.has(organization.id))
      .map((organization) => ({
        value: organization.id,
        label: organization.name,
      }));
  },

  createOrganization: async (organization: CreateOrganizationRequest) => {
    const organizations = get().organizations;
    set({ organizations: organizations.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.createOrganization(organization),
    );

    if (error) {
      set({ organizations: organizations.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "organizationCreateFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "organizationCreated"),
    );
    get().getOrganizations();
  },

  updateOrganization: async (
    id: string,
    organization: UpdateOrganizationRequest,
  ) => {
    const organizations = get().organizations;
    set({ organizations: organizations.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.updateOrganization(id, organization),
    );

    if (error) {
      set({ organizations: organizations.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "organizationUpdateFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "organizationUpdated"),
    );

    get().getOrganizations();
  },

  getServiceAccounts: async (query?: Query) => {
    const serviceAccounts = get().serviceAccounts;
    set({ serviceAccounts: serviceAccounts.setLoading() });
    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.getServiceAccounts(query),
    );
    if (error) {
      set({ serviceAccounts: serviceAccounts.setError(message) });
      return;
    }
    set({ serviceAccounts: serviceAccounts.setSuccess(data) });
  },

  searchServiceAccountOptions: async (
    query: string,
  ): Promise<SelectOption[]> => {
    const { data, error } = await localizedApiResponse(
      get().language,
      Service.admin.getServiceAccounts({
        q: query || undefined,
        limit: 20,
      }),
    );
    if (error) return [];

    return (data?.data ?? []).map((serviceAccount) => ({
      value: serviceAccount.id,
      label: `${serviceAccount.name} (${serviceAccount.id})`,
    }));
  },

  createServiceAccount: async (serviceAccount: CreateServiceAccountRequest) => {
    const serviceAccounts = get().serviceAccounts;
    set({ serviceAccounts: serviceAccounts.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.createServiceAccount(serviceAccount),
    );

    if (error) {
      set({ serviceAccounts: serviceAccounts.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ??
          notificationText(get().language, "serviceAccountCreateFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "serviceAccountCreated"),
    );
    get().getServiceAccounts();
  },

  updateServiceAccount: async (
    id: string,
    serviceAccount: UpdateServiceAccountRequest,
  ) => {
    const serviceAccounts = get().serviceAccounts;
    set({ serviceAccounts: serviceAccounts.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.updateServiceAccount(id, serviceAccount),
    );

    if (error) {
      set({ serviceAccounts: serviceAccounts.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ??
          notificationText(get().language, "serviceAccountUpdateFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "serviceAccountUpdated"),
    );

    get().getServiceAccounts();
  },

  deleteServiceAccount: async (id: string) => {
    const serviceAccounts = get().serviceAccounts;
    set({ serviceAccounts: serviceAccounts.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.deleteServiceAccount(id),
    );

    if (error) {
      set({ serviceAccounts: serviceAccounts.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ??
          notificationText(get().language, "serviceAccountDeleteFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      data?.message ??
        message ??
        notificationText(get().language, "serviceAccountDeleted"),
    );

    get().getServiceAccounts();
  },

  getUsers: async (query?: Query) => {
    const usersQuery = query ?? get().usersQuery;
    const users = get().users;
    set({ users: users.setLoading(), usersQuery });
    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.getUsers(usersQuery),
    );
    if (error) {
      set({ users: users.setError(message) });
      return;
    }
    set({ users: users.setSuccess(data) });
  },

  searchUserOptions: async (query: string): Promise<SelectOption[]> => {
    const { data, error } = await localizedApiResponse(
      get().language,
      Service.admin.getUsers({
        q: query || undefined,
        limit: 20,
      }),
    );
    if (error) return [];

    return (data?.data ?? []).map((user) => ({
      value: user.id,
      label: `${user.email} (${user.nickname})`,
    }));
  },

  getSuperAdminUsers: async () => {
    const superAdminUsers = get().superAdminUsers;
    set({ superAdminUsers: superAdminUsers.setLoading() });
    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.getSuperAdminUsers(),
    );
    if (error) {
      set({ superAdminUsers: superAdminUsers.setError(message) });
      return;
    }
    set({ superAdminUsers: superAdminUsers.setSuccess(data) });
  },

  createUser: async (user: CreateUserRequest) => {
    const users = get().users;
    set({ users: users.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.createUser(user),
    );

    if (error) {
      set({ users: users.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "userCreateFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "userCreated"),
    );
    get().getUsers(get().usersQuery);
  },

  inviteUser: async (user: CreateUserInvitationRequest) => {
    const users = get().users;
    set({ users: users.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.inviteUser(user),
    );

    if (error) {
      set({ users: users.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "userInvitationSendFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      data?.message ??
        message ??
        notificationText(get().language, "userInvitationSent"),
    );
    get().getUsers(get().usersQuery);
  },

  updateUser: async (id: string, user: UpdateUserRequest) => {
    const users = get().users;
    set({ users: users.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.updateUser(id, user),
    );

    if (error) {
      set({ users: users.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "userUpdateFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "userUpdated"),
    );

    get().getUsers(get().usersQuery);
  },

  deleteUser: async (id: string) => {
    const users = get().users;
    set({ users: users.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.deleteUser(id),
    );

    if (error) {
      set({ users: users.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "userDeleteFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      data?.message ??
        message ??
        notificationText(get().language, "userDeleted"),
    );

    get().getUsers(get().usersQuery);
  },

  updateUserAttrs: async (id: string, attrs: JsonValue) => {
    const users = get().users;
    set({ users: users.setLoading() });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.updateUserAttrs(id, { attrs }),
    );

    if (error) {
      set({ users: users.setError(message) });

      showStoreNotification(
        get().language,
        "error",
        message ?? notificationText(get().language, "userAttrsUpdateFailed"),
      );
      return;
    }

    showStoreNotification(
      get().language,
      "success",
      notificationText(get().language, "userAttrsUpdated"),
    );

    get().getUsers(get().usersQuery);
  },

  getUserOrganizations: async (id: string) => {
    const userOrganizations = get().userOrganizations;
    if (userOrganizations.meta !== id) {
      userOrganizations.setData(null);
    }
    set({
      userOrganizations: userOrganizations.setMeta(id).setLoading(),
    });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.getUserOrganizations(id),
    );
    if (get().userOrganizations.meta !== id) return;

    if (error) {
      set({
        userOrganizations: userOrganizations.setError(message),
      });
      showStoreNotification(
        get().language,
        "error",
        message ??
          notificationText(get().language, "userOrganizationsLoadFailed"),
      );
      return;
    }

    set({
      userOrganizations: userOrganizations.setSuccess(data ?? [], id),
    });
  },

  getUserOrganizationOptions: async (id: string): Promise<SelectOption[]> => {
    const { data, error } = await localizedApiResponse(
      get().language,
      Service.admin.getUserOrganizations(id),
    );
    if (error) return [];

    return (data ?? []).map((organization) => ({
      value: organization.id,
      label: organization.name,
    }));
  },

  upsertUserOrganization: async (id, orgId, role) => {
    const userOrganizations = get().userOrganizations;
    set({
      userOrganizations: userOrganizations.setMeta(id).setLoading(),
    });

    const { error, message } = await localizedApiResponse(
      get().language,
      Service.admin.addUserToOrganization(id, orgId, { role }),
    );
    if (error) {
      if (get().userOrganizations.meta === id) {
        set({
          userOrganizations: userOrganizations.setError(message),
        });
      }
      showStoreNotification(
        get().language,
        "error",
        message ??
          notificationText(get().language, "userOrganizationUpdateFailed"),
      );
      return false;
    }

    if (get().userOrganizations.meta === id) {
      await get().getUserOrganizations(id);
    }
    return true;
  },

  removeUserOrganization: async (id, orgId) => {
    const userOrganizations = get().userOrganizations;
    set({
      userOrganizations: userOrganizations.setMeta(id).setLoading(),
    });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.removeUserFromOrganization(id, orgId),
    );
    if (error) {
      if (get().userOrganizations.meta === id) {
        set({
          userOrganizations: userOrganizations.setError(message),
        });
      }
      showStoreNotification(
        get().language,
        "error",
        message ??
          notificationText(get().language, "userOrganizationRemoveFailed"),
      );
      return false;
    }

    showStoreNotification(
      get().language,
      "success",
      data?.message ??
        message ??
        notificationText(get().language, "userOrganizationRemoved"),
    );
    if (get().userOrganizations.meta === id) {
      await get().getUserOrganizations(id);
    }
    return true;
  },

  getOutboxEvents: async (query) => {
    const outboxEvents = get().outboxEvents;
    set({
      outboxEvents: outboxEvents.setLoading(),
      outboxEventsQuery: query ?? {},
    });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.getOutboxEvents(query),
    );
    if (error) {
      set({
        outboxEvents: outboxEvents.setError(message),
      });
      return;
    }

    set({ outboxEvents: outboxEvents.setSuccess(data) });
  },

  getOutboxEvent: async (eventId) => {
    const selected = get().selectedOutboxEvent;
    set({ selectedOutboxEvent: selected.setLoading() });

    const { data, error, message } = await localizedApiResponse(
      get().language,
      Service.admin.getOutboxEvent(eventId),
    );
    if (error) {
      set({
        selectedOutboxEvent: selected.setError(message),
      });
      return;
    }

    set({ selectedOutboxEvent: selected.setSuccess(data) });
  },
});

export default create(devtools(store));
