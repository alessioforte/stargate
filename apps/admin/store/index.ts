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
  OAuthClient,
  Organization,
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
  ValidateAccessControlRulesRequest,
  ValidateAccessControlRulesResponse,
  EvaluateAccessControlRequest,
  EvaluateAccessControlResponse,
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
  superAdminUsers: new StoreItem<List<User>>(null),
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
    try {
      await logoutAndRedirect(returnPath);
    } catch (error: unknown) {
      const message = errorMessage(error, "Unable to restart admin auth");
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
    } catch (error: unknown) {
      const message = errorMessage(error, "Unable to start admin auth");
      set({
        adminError: message,
        adminReturnPath: returnPath,
        adminSessionStatus: "error",
        adminStatus: null,
      });
    }
  },

  setLanguage: (lang: "en" | "it") => {
    set({ language: lang });
    Settings.set("language", lang);
  },

  getAdminKeys: async (query?: Query) => {
    const adminKeys = get().adminKeys;
    set({ adminKeys: adminKeys.setLoading() });
    const { data, error, message } = await Service.admin.getAdminKeys(query);
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

    const { data, error, message } =
      await Service.admin.createAdminKey(adminKey);

    if (error) {
      set({ adminKeys: adminKeys.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to create admin key",
      });
      return null;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "Admin key created successfully",
    });
    get().getAdminKeys();
    return data;
  },

  updateAdminKeyPermissions: async (
    id: string,
    adminKey: UpdateAdminKeyPermissionsRequest,
  ) => {
    const adminKeys = get().adminKeys;
    set({ adminKeys: adminKeys.setLoading() });

    const { error, message } = await Service.admin.updateAdminKeyPermissions(
      id,
      adminKey,
    );

    if (error) {
      set({ adminKeys: adminKeys.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to update admin key permissions",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "Admin key permissions updated successfully",
    });

    get().getAdminKeys();
  },

  revokeAdminKey: async (id: string) => {
    const adminKeys = get().adminKeys;
    set({ adminKeys: adminKeys.setLoading() });

    const { error, message } = await Service.admin.revokeAdminKey(id);

    if (error) {
      set({ adminKeys: adminKeys.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to revoke admin key",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "Admin key revoked successfully",
    });

    get().getAdminKeys();
  },

  getApiKeys: async (query?: ApiKeyQuery) => {
    const apiKeys = get().apiKeys;
    set({ apiKeys: apiKeys.setLoading() });
    const { data, error, message } = await Service.admin.getApiKeys(query);
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

    const { data, error, message } = await Service.admin.createApiKey(apiKey);

    if (error) {
      set({ apiKeys: apiKeys.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to create API key",
      });
      return null;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "API key created successfully",
    });
    get().getApiKeys();
    return data;
  },

  updateApiKeyAttrs: async (id: string, attrs: JsonValue) => {
    const apiKeys = get().apiKeys;
    set({ apiKeys: apiKeys.setLoading() });

    const { error, message } = await Service.admin.updateApiKeyAttrs(id, {
      attrs,
    });

    if (error) {
      set({ apiKeys: apiKeys.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to update API key attrs",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "API key attrs updated successfully",
    });

    get().getApiKeys();
  },

  revokeApiKey: async (id: string) => {
    const apiKeys = get().apiKeys;
    set({ apiKeys: apiKeys.setLoading() });

    const { error, message } = await Service.admin.revokeApiKey(id);

    if (error) {
      set({ apiKeys: apiKeys.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to revoke API key",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "API key revoked successfully",
    });

    get().getApiKeys();
  },

  deleteApiKey: async (id: string) => {
    const apiKeys = get().apiKeys;
    set({ apiKeys: apiKeys.setLoading() });

    const { error, message } = await Service.admin.deleteApiKey(id);

    if (error) {
      set({ apiKeys: apiKeys.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to delete API key",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "API key deleted successfully",
    });

    get().getApiKeys();
  },

  getConfigurations: async () => {
    const configuration = get().configuration;
    set({ configuration: configuration.setLoading() });
    const { data, error, message } = await Service.admin.getConfigurations();
    if (error) {
      set({ configuration: configuration.setError(message) });
      return;
    }
    set({ configuration: configuration.setSuccess(data) });
  },

  updateConfigurations: async (configuration: Configuration) => {
    const configurationItem = get().configuration;
    set({ configuration: configurationItem.setLoading() });

    const { error, message } =
      await Service.admin.updateConfigurations(configuration);

    if (error) {
      set({ configuration: configurationItem.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to update gateway configuration",
      });
      return false;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "Gateway configuration updated successfully",
    });

    set({ configuration: configurationItem.setSuccess(configuration) });
    return true;
  },

  getAccessControlRules: async () => {
    const accessControlRules = get().accessControlRules;
    set({ accessControlRules: accessControlRules.setLoading() });

    const { data, error, message } =
      await Service.admin.getAccessControlRules();
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

    const { data, error, message } =
      await Service.admin.updateAccessControlRules(request);

    if (error) {
      set({ accessControlRules: accessControlRules.setError(message) });
      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to save access control policies",
      });
      return null;
    }

    set({
      accessControlRules: accessControlRules.setSuccess(data),
      accessControlValidation: get().accessControlValidation.setSuccess(
        data ? accessControlValidationFromRules(data) : null,
      ),
    });

    showNotification({
      type: "success",
      title: "Success",
      message: "Access control policies saved successfully",
    });

    return data;
  },

  validateAccessControlRules: async (
    request: ValidateAccessControlRulesRequest,
  ) => {
    const accessControlValidation = get().accessControlValidation;
    set({
      accessControlValidation: accessControlValidation.setLoading(),
    });

    const { data, error, message } =
      await Service.admin.validateAccessControlRules(request);

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

    const { data, error, message } =
      await Service.admin.evaluateAccessControlRules(request);

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
    const { data, error, message } = await Service.admin.getOAuthClients(query);
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

    const { data, error, message } =
      await Service.admin.createOAuthClient(oauthClient);

    if (error) {
      set({ oauthClients: oauthClients.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to create OAuth client",
      });
      return null;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "OAuth client created successfully",
    });
    get().getOAuthClients();
    return data;
  },

  updateOAuthClient: async (
    clientId: string,
    oauthClient: UpdateOAuthClientRequest,
  ) => {
    const oauthClients = get().oauthClients;
    set({ oauthClients: oauthClients.setLoading() });

    const { error, message } = await Service.admin.updateOAuthClient(
      clientId,
      oauthClient,
    );

    if (error) {
      set({ oauthClients: oauthClients.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to update OAuth client",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "OAuth client updated successfully",
    });

    get().getOAuthClients();
  },

  enableOAuthClient: async (clientId: string) => {
    const oauthClients = get().oauthClients;
    set({ oauthClients: oauthClients.setLoading() });

    const { error, message } = await Service.admin.enableOAuthClient(clientId);

    if (error) {
      set({ oauthClients: oauthClients.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to enable OAuth client",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "OAuth client enabled successfully",
    });

    get().getOAuthClients();
  },

  disableOAuthClient: async (clientId: string) => {
    const oauthClients = get().oauthClients;
    set({ oauthClients: oauthClients.setLoading() });

    const { error, message } = await Service.admin.disableOAuthClient(clientId);

    if (error) {
      set({ oauthClients: oauthClients.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to disable OAuth client",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "OAuth client disabled successfully",
    });

    get().getOAuthClients();
  },

  deleteOAuthClient: async (clientId: string) => {
    const oauthClients = get().oauthClients;
    set({ oauthClients: oauthClients.setLoading() });

    const { error, message } = await Service.admin.deleteOAuthClient(clientId);

    if (error) {
      set({ oauthClients: oauthClients.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to delete OAuth client",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "OAuth client deleted successfully",
    });

    get().getOAuthClients();
  },

  rotateOAuthClientSecret: async (
    clientId: string,
  ): Promise<RotateOAuthClientSecretResponse | null> => {
    const oauthClients = get().oauthClients;
    set({ oauthClients: oauthClients.setLoading() });

    const { data, error, message } =
      await Service.admin.rotateOAuthClientSecret(clientId);

    if (error) {
      set({ oauthClients: oauthClients.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to rotate OAuth client secret",
      });
      return null;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "OAuth client secret rotated successfully",
    });

    get().getOAuthClients();
    return data;
  },

  getOrganizations: async (query?: Query) => {
    const organizations = get().organizations;
    set({ organizations: organizations.setLoading() });
    const { data, error, message } =
      await Service.admin.getOrganizations(query);
    if (error) {
      set({ organizations: organizations.setError(message) });
      return;
    }
    set({ organizations: organizations.setSuccess(data) });
  },

  createOrganization: async (organization: CreateOrganizationRequest) => {
    const organizations = get().organizations;
    set({ organizations: organizations.setLoading() });

    const { error, message } =
      await Service.admin.createOrganization(organization);

    if (error) {
      set({ organizations: organizations.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to create organization",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "Organization created successfully",
    });
    get().getOrganizations();
  },

  updateOrganization: async (
    id: string,
    organization: UpdateOrganizationRequest,
  ) => {
    const organizations = get().organizations;
    set({ organizations: organizations.setLoading() });

    const { error, message } = await Service.admin.updateOrganization(
      id,
      organization,
    );

    if (error) {
      set({ organizations: organizations.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to update organization",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "Organization updated successfully",
    });

    get().getOrganizations();
  },

  getServiceAccounts: async (query?: Query) => {
    const serviceAccounts = get().serviceAccounts;
    set({ serviceAccounts: serviceAccounts.setLoading() });
    const { data, error, message } =
      await Service.admin.getServiceAccounts(query);
    if (error) {
      set({ serviceAccounts: serviceAccounts.setError(message) });
      return;
    }
    set({ serviceAccounts: serviceAccounts.setSuccess(data) });
  },

  createServiceAccount: async (serviceAccount: CreateServiceAccountRequest) => {
    const serviceAccounts = get().serviceAccounts;
    set({ serviceAccounts: serviceAccounts.setLoading() });

    const { error, message } =
      await Service.admin.createServiceAccount(serviceAccount);

    if (error) {
      set({ serviceAccounts: serviceAccounts.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to create service account",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "Service account created successfully",
    });
    get().getServiceAccounts();
  },

  updateServiceAccount: async (
    id: string,
    serviceAccount: UpdateServiceAccountRequest,
  ) => {
    const serviceAccounts = get().serviceAccounts;
    set({ serviceAccounts: serviceAccounts.setLoading() });

    const { error, message } = await Service.admin.updateServiceAccount(
      id,
      serviceAccount,
    );

    if (error) {
      set({ serviceAccounts: serviceAccounts.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to update service account",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "Service account updated successfully",
    });

    get().getServiceAccounts();
  },

  deleteServiceAccount: async (id: string) => {
    const serviceAccounts = get().serviceAccounts;
    set({ serviceAccounts: serviceAccounts.setLoading() });

    const { error, message } = await Service.admin.deleteServiceAccount(id);

    if (error) {
      set({ serviceAccounts: serviceAccounts.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to delete service account",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "Service account deleted successfully",
    });

    get().getServiceAccounts();
  },

  getUsers: async (query?: Query) => {
    const usersQuery = query ?? get().usersQuery;
    const users = get().users;
    set({ users: users.setLoading(), usersQuery });
    const { data, error, message } = await Service.admin.getUsers(usersQuery);
    if (error) {
      set({ users: users.setError(message) });
      return;
    }
    set({ users: users.setSuccess(data) });
  },

  getSuperAdminUsers: async () => {
    const superAdminUsers = get().superAdminUsers;
    set({ superAdminUsers: superAdminUsers.setLoading() });
    const { data, error, message } = await Service.admin.getSuperAdminUsers();
    if (error) {
      set({ superAdminUsers: superAdminUsers.setError(message) });
      return;
    }
    set({ superAdminUsers: superAdminUsers.setSuccess(data) });
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
    get().getUsers(get().usersQuery);
  },

  inviteUser: async (user: CreateUserInvitationRequest) => {
    const users = get().users;
    set({ users: users.setLoading() });

    const { error, message } = await Service.admin.inviteUser(user);

    if (error) {
      set({ users: users.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to send user invitation",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: message ?? "User invitation sent successfully",
    });
    get().getUsers(get().usersQuery);
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

    get().getUsers(get().usersQuery);
  },

  deleteUser: async (id: string) => {
    const users = get().users;
    set({ users: users.setLoading() });

    const { error, message } = await Service.admin.deleteUser(id);

    if (error) {
      set({ users: users.setError(message) });

      showNotification({
        type: "error",
        title: "Error",
        message: message ?? "Failed to delete user",
      });
      return;
    }

    showNotification({
      type: "success",
      title: "Success",
      message: "User deleted successfully",
    });

    get().getUsers(get().usersQuery);
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

    get().getUsers(get().usersQuery);
  },
});

export default create(devtools(store));
