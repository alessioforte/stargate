import { StoreItem } from "./item";
import type {
  AccessControlRulesResponse,
  AdminKey,
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
  CreateOrganizationRequest,
  UpdateOrganizationRequest,
  UpdateAdminKeyPermissionsRequest,
  RotateOAuthClientSecretResponse,
  ServiceAccount,
  CreateServiceAccountRequest,
  UpdateServiceAccountRequest,
  UpdateOAuthClientRequest,
  User,
  CreateUserRequest,
  UpdateUserRequest,
  Query,
  JsonValue,
  UpdateAccessControlRulesRequest,
  ValidateAccessControlRulesRequest,
  ValidateAccessControlRulesResponse,
  EvaluateAccessControlRequest,
  EvaluateAccessControlResponse,
} from "@/services/types";

export interface AppMessage {
  type: "info" | "success" | "warning" | "error";
  text: string;
}

export type AdminSessionStatus = "checking" | "redirecting" | "ready" | "error";

export interface AdminStatus {
  apiName: string;
  databaseStatus: string;
  redisStatus: string;
  status: string;
  version: string;
}

export type AdminSessionResult =
  | { status: "ready" }
  | { status: "redirecting" }
  | { status: "error"; message: string };

export interface State {
  adminError: string | null;
  adminReturnPath: string;
  adminSessionStatus: AdminSessionStatus;
  adminStatus: AdminStatus | null;
  message: AppMessage | null;
  loading: boolean;
  theme: "light" | "dark" | "system";
  language: string;

  accessControlRules: StoreItem<AccessControlRulesResponse>;
  accessControlValidation: StoreItem<ValidateAccessControlRulesResponse>;
  accessControlEvaluation: StoreItem<EvaluateAccessControlResponse>;
  adminKeys: StoreItem<List<AdminKey>>;
  apiKeys: StoreItem<List<ApiKey>>;
  configuration: StoreItem<Configuration>;
  oauthClients: StoreItem<List<OAuthClient>>;
  organizations: StoreItem<List<Organization>>;
  serviceAccounts: StoreItem<List<ServiceAccount>>;
  users: StoreItem<List<User>>;
}

export interface Actions {
  clearAdminError: () => void;
  ensureAdminSession: (returnPath: string) => Promise<AdminSessionResult>;
  logout: (returnPath: string) => Promise<void>;
  signIn: (returnPath: string) => Promise<void>;
  setTheme: (theme: "light" | "dark" | "system") => void;
  setLanguage: (lang: "en" | "it") => void;

  getAdminKeys: (query?: Query) => Promise<void>;
  createAdminKey: (
    adminKey: CreateAdminKeyRequest,
  ) => Promise<CreateAdminKeyResponse | null>;
  updateAdminKeyPermissions: (
    id: string,
    adminKey: UpdateAdminKeyPermissionsRequest,
  ) => Promise<void>;
  revokeAdminKey: (id: string) => Promise<void>;

  getApiKeys: (query?: ApiKeyQuery) => Promise<void>;
  createApiKey: (
    apiKey: CreateApiKeyRequest,
  ) => Promise<CreateApiKeyResponse | null>;
  updateApiKeyAttrs: (id: string, attrs: JsonValue) => Promise<void>;
  revokeApiKey: (id: string) => Promise<void>;
  deleteApiKey: (id: string) => Promise<void>;

  getConfigurations: () => Promise<void>;
  updateConfigurations: (configuration: Configuration) => Promise<boolean>;

  getAccessControlRules: () => Promise<void>;
  updateAccessControlRules: (
    request: UpdateAccessControlRulesRequest,
  ) => Promise<AccessControlRulesResponse | null>;
  validateAccessControlRules: (
    request: ValidateAccessControlRulesRequest,
  ) => Promise<ValidateAccessControlRulesResponse | null>;
  evaluateAccessControlRules: (
    request: EvaluateAccessControlRequest,
  ) => Promise<EvaluateAccessControlResponse | null>;

  getOAuthClients: (query?: Query) => Promise<void>;
  createOAuthClient: (
    oauthClient: CreateOAuthClientRequest,
  ) => Promise<CreateOAuthClientResponse | null>;
  updateOAuthClient: (
    clientId: string,
    oauthClient: UpdateOAuthClientRequest,
  ) => Promise<void>;
  enableOAuthClient: (clientId: string) => Promise<void>;
  disableOAuthClient: (clientId: string) => Promise<void>;
  deleteOAuthClient: (clientId: string) => Promise<void>;
  rotateOAuthClientSecret: (
    clientId: string,
  ) => Promise<RotateOAuthClientSecretResponse | null>;

  getOrganizations: (query?: Query) => Promise<void>;
  createOrganization: (
    organization: CreateOrganizationRequest,
  ) => Promise<void>;
  updateOrganization: (
    id: string,
    organization: UpdateOrganizationRequest,
  ) => Promise<void>;

  getServiceAccounts: (query?: Query) => Promise<void>;
  createServiceAccount: (
    serviceAccount: CreateServiceAccountRequest,
  ) => Promise<void>;
  updateServiceAccount: (
    id: string,
    serviceAccount: UpdateServiceAccountRequest,
  ) => Promise<void>;
  deleteServiceAccount: (id: string) => Promise<void>;

  getUsers: (query?: Query) => Promise<void>;
  createUser: (user: CreateUserRequest) => Promise<void>;
  updateUser: (id: string, user: UpdateUserRequest) => Promise<void>;
  deleteUser: (id: string) => Promise<void>;
  updateUserAttrs: (id: string, attrs: JsonValue) => Promise<void>;
}
