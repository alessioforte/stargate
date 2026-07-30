import { StoreItem } from "./item";
import type {
  AccessControlRulesResponse,
  AdminHealth,
  AdminKey,
  AdminMe,
  AdminOverview,
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
  ListOutboxEventsQuery,
  OAuthClient,
  Organization,
  CreateOrganizationRequest,
  UpdateOrganizationRequest,
  OutboxEvent,
  UpdateAdminKeyPermissionsRequest,
  RotateOAuthClientSecretResponse,
  ServiceAccount,
  CreateServiceAccountRequest,
  UpdateServiceAccountRequest,
  UpdateOAuthClientRequest,
  UserOrganization,
  User,
  CreateUserRequest,
  CreateUserInvitationRequest,
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
  status: string;
  storeStatus: string;
  version: string;
}

export type AdminSessionResult =
  | { status: "ready" }
  | { status: "redirecting" }
  | { status: "error"; message: string };

export interface OAuthCallbackParams {
  code: string | null;
  error: string | null;
  errorDescription: string | null;
  state: string | null;
}

export type OAuthCallbackResult =
  | { status: "success"; returnPath: string }
  | { status: "redirecting" }
  | { status: "error"; message: string };

export interface SelectOption {
  label: string;
  value: string;
}

export interface State {
  adminError: string | null;
  adminReturnPath: string;
  adminSessionStatus: AdminSessionStatus;
  adminHealth: StoreItem<AdminHealth>;
  adminMe: AdminMe | null;
  adminOverview: StoreItem<AdminOverview>;
  adminStatus: AdminStatus | null;
  message: AppMessage | null;
  loading: boolean;
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
  usersQuery: Query;
  userOrganizations: StoreItem<UserOrganization[], string>;
  superAdminUsers: StoreItem<List<User>>;
  outboxEvents: StoreItem<List<OutboxEvent>>;
  outboxEventsQuery: ListOutboxEventsQuery;
  selectedOutboxEvent: StoreItem<OutboxEvent>;
}

export interface Actions {
  clearAdminError: () => void;
  completeOAuthCallback: (
    params: OAuthCallbackParams,
  ) => Promise<OAuthCallbackResult>;
  ensureAdminSession: (returnPath: string) => Promise<AdminSessionResult>;
  getAdminHealth: () => Promise<void>;
  getAdminOverview: () => Promise<void>;
  logout: (returnPath: string) => Promise<void>;
  signIn: (returnPath: string) => Promise<AdminSessionResult>;
  setLanguage: (lang: "en" | "it") => void;
  loadApiMessages: (locale?: "en" | "it") => Promise<void>;

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
  searchOrganizationOptions: (
    query: string,
    excludedIds?: string[],
  ) => Promise<SelectOption[]>;
  createOrganization: (
    organization: CreateOrganizationRequest,
  ) => Promise<void>;
  updateOrganization: (
    id: string,
    organization: UpdateOrganizationRequest,
  ) => Promise<void>;

  getServiceAccounts: (query?: Query) => Promise<void>;
  searchServiceAccountOptions: (query: string) => Promise<SelectOption[]>;
  createServiceAccount: (
    serviceAccount: CreateServiceAccountRequest,
  ) => Promise<void>;
  updateServiceAccount: (
    id: string,
    serviceAccount: UpdateServiceAccountRequest,
  ) => Promise<void>;
  deleteServiceAccount: (id: string) => Promise<void>;

  getUsers: (query?: Query) => Promise<void>;
  searchUserOptions: (query: string) => Promise<SelectOption[]>;
  getSuperAdminUsers: (query?: Query) => Promise<void>;
  createUser: (user: CreateUserRequest) => Promise<void>;
  inviteUser: (user: CreateUserInvitationRequest) => Promise<void>;
  updateUser: (id: string, user: UpdateUserRequest) => Promise<void>;
  deleteUser: (id: string) => Promise<void>;
  updateUserAttrs: (id: string, attrs: JsonValue) => Promise<void>;
  getUserOrganizations: (id: string) => Promise<void>;
  getUserOrganizationOptions: (id: string) => Promise<SelectOption[]>;
  upsertUserOrganization: (
    id: string,
    orgId: string,
    role: string,
  ) => Promise<boolean>;
  removeUserOrganization: (id: string, orgId: string) => Promise<boolean>;

  getOutboxEvents: (query?: ListOutboxEventsQuery) => Promise<void>;
  getOutboxEvent: (eventId: string) => Promise<void>;
}
