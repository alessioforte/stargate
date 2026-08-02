export type JsonPrimitive = string | number | boolean | null;
export type JsonValue =
  | JsonPrimitive
  | JsonValue[]
  | { [key: string]: JsonValue };

export type ApiMessageParams = Record<string, string | number | boolean | null>;

export interface ApiMessages {
  errors: Record<string, string>;
  messages: Record<string, string>;
}

export interface ApiMessageCatalog {
  schema: "stargate/i18n/v1";
  locale: string;
  fallbackLocale: string;
  version: string;
  messages: ApiMessages;
}

export interface List<T> {
  data: T[];
  limit: number;
  offset: number;
  total: number;
}

export interface PaginationQuery {
  limit?: number;
  offset?: number;
}

export interface Query extends PaginationQuery {
  q?: string;
}

export interface MessageResponse {
  code: string;
  message: string;
  params?: ApiMessageParams;
}

export interface ComponentStatus {
  detail?: string | null;
  latencyMs: number;
  status: string;
}

export interface StoreStats {
  cacheHitRatio: number;
  estimatedMemoryBytes: number;
  totalKeys: number;
}

export interface AdminHealth {
  checkedAt: string;
  database: ComponentStatus;
  databaseBackend: string;
  name: string;
  redis?: ComponentStatus;
  runtimeProfile: string;
  stateBackend: string;
  status: string;
  store?: StoreStats;
  version: string;
}

export interface AdminMe {
  user: {
    email: string;
    id: string;
    name: string;
    picture: string | null;
  };
  authentication: {
    authenticatedAt: string | null;
    clientId: string | null;
    kind: "oauth" | "session";
    sessionId: string | null;
    scopes: string[];
    tokenExpiresAt: string;
    tokenIssuedAt: string;
  };
  authorization: {
    grants: string[];
  };
}

export interface AdminOverview {
  generatedAt: string;
  resources: {
    users: { total: number };
    organizations: { total: number };
    serviceAccounts: { total: number };
    oauthClients: {
      disabled: number;
      enabled: number;
      total: number;
    };
    apiKeys: {
      active: number;
      revoked: number;
      total: number;
    };
    adminKeys: {
      active: number;
      revoked: number;
      total: number;
    };
  };
  audit: {
    mode: "disabled" | "durable_only" | "relay";
    oldestPendingAt: string | null;
    pending: number;
  };
  gateway: {
    configVersion: number;
    policyRevision: string | null;
    routers: number;
    services: number;
    targets: number;
    upstreams: number;
  };
  sessions: {
    activeSessions: number;
    activeUsers: number;
  };
}

export interface AdminSessionUser {
  email: string | null;
  id: string;
  name: string | null;
}

export interface AdminSession {
  authenticatedAt: string;
  clientIds: string[];
  current: boolean;
  expiresAt: string;
  id: string;
  lastSeenAt: string;
  organizationId: string | null;
  user: AdminSessionUser;
}

export interface ListAdminSessionsQuery extends PaginationQuery {
  clientId?: string;
  organizationId?: string;
  userId?: string;
}

export interface SessionRevocationResponse {
  revoked: boolean;
  sessionId: string;
  userId: string;
}

export interface UserSessionsRevocationResponse {
  revokedSessions: number;
  userId: string;
}

export interface ConfigurationsQuery {
  format?: string;
}

export type Configuration = JsonValue;

export type AccessControlEffect = "allow" | "deny";
export type AccessControlResourceAction =
  | "READ"
  | "WRITE"
  | "DELETE"
  | "CREATE"
  | "UPDATE"
  | "EXECUTE"
  | "ADMIN"
  | "*"
  | "ANY";

export interface AccessControlRule {
  action?: AccessControlResourceAction | null;
  condition?: string | null;
  description?: string | null;
  effect: AccessControlEffect;
  id?: string | null;
  line: number;
  resource: string;
  statement: string;
  subject: string;
}

export interface PolicyDiagnostic {
  line: number;
  message: string;
  statement: string;
}

export interface AccessControlRulesResponse {
  content: string;
  diagnostics: PolicyDiagnostic[];
  revision: string;
  rules: AccessControlRule[];
  valid: boolean;
}

export interface UpdateAccessControlRulesRequest {
  content: string;
  revision: string;
}

export interface ValidateAccessControlRulesRequest {
  content: string;
}

export interface ValidateAccessControlRulesResponse {
  diagnostics: PolicyDiagnostic[];
  rules: AccessControlRule[];
  valid: boolean;
}

export interface EvaluateAccessControlRequest {
  action?: AccessControlResourceAction | null;
  context?: { [key: string]: JsonValue };
  resource: string;
  subject: string;
}

export interface EvaluateAccessControlResponse {
  allowCount: number;
  allowed: boolean;
  appliedPolicies: number[];
  denyCount: number;
  matchedPolicies: number[];
}

export const AdminKeyPermission = {
  AccessControl: "access_control",
  Users: "users",
  Organizations: "organizations",
  ApiKeys: "api_keys",
  OAuthClients: "oauth_clients",
  ServiceAccounts: "service_accounts",
  Configurations: "configurations",
} as const;

export type AdminKeyPermission =
  (typeof AdminKeyPermission)[keyof typeof AdminKeyPermission];

export type AdminKeyStoredPermission = AdminKeyPermission | "super_admin";

export interface AdminKey {
  createdAt: string;
  id: string;
  label?: string | null;
  permissions: AdminKeyStoredPermission[];
  revoked: boolean;
  updatedAt: string;
}

export interface CreateAdminKeyRequest {
  label?: string | null;
  permissions: AdminKeyPermission[];
}

export interface CreateAdminKeyResponse extends AdminKey {
  apiKey: string;
}

export interface UpdateAdminKeyPermissionsRequest {
  permissions: AdminKeyPermission[];
}

export type ApiKeyOwnerType = "user" | "service_account";

export interface ApiKeyQuery extends PaginationQuery {
  q?: string;
  ownerType?: ApiKeyOwnerType;
  serviceAccountId?: string;
  userId?: string;
}

export interface ApiKey {
  attrs: JsonValue;
  createdAt: string;
  id: string;
  label: string;
  revoked: boolean;
  updatedAt: string;
}

export interface CreateApiKeyRequest {
  attrs?: JsonValue;
  label: string;
  serviceAccountId?: string | null;
  userId?: string | null;
  /** Optional org binding for user API keys; the user must be a member. */
  orgId?: string | null;
}

export interface CreateApiKeyResponse extends ApiKey {
  apiKey: string;
}

export interface ApiKeyAttrsRequest {
  attrs: JsonValue;
}

export const OAuthGrantType = {
  AuthorizationCode: "authorization_code",
  ClientCredentials: "client_credentials",
  RefreshToken: "refresh_token",
} as const;

export type OAuthGrantType =
  (typeof OAuthGrantType)[keyof typeof OAuthGrantType];

export const OAuthResponseType = {
  Code: "code",
} as const;

export type OAuthResponseType =
  (typeof OAuthResponseType)[keyof typeof OAuthResponseType];

export const OAuthTokenEndpointAuthMethod = {
  None: "none",
  ClientSecretBasic: "client_secret_basic",
  ClientSecretPost: "client_secret_post",
} as const;

export type OAuthTokenEndpointAuthMethod =
  (typeof OAuthTokenEndpointAuthMethod)[keyof typeof OAuthTokenEndpointAuthMethod];

export interface OAuthClient {
  attrs: JsonValue;
  audiences: string[];
  clientId: string;
  createdAt: string;
  description?: string | null;
  enabled: boolean;
  grantTypes: OAuthGrantType[];
  name: string;
  redirectUris: string[];
  responseTypes: OAuthResponseType[];
  scopes: string[];
  tokenEndpointAuthMethod: OAuthTokenEndpointAuthMethod;
  updatedAt: string;
}

export interface CreateOAuthClientRequest {
  attrs?: JsonValue;
  audiences?: string[] | null;
  clientId?: string | null;
  description?: string | null;
  grantTypes: OAuthGrantType[];
  name: string;
  redirectUris?: string[] | null;
  responseTypes?: OAuthResponseType[] | null;
  scopes?: string[] | null;
  tokenEndpointAuthMethod?: OAuthTokenEndpointAuthMethod | null;
}

export interface CreateOAuthClientResponse extends OAuthClient {
  clientSecret?: string | null;
}

export interface UpdateOAuthClientRequest {
  attrs?: JsonValue;
  audiences?: string[];
  description?: string | null;
  grantTypes: OAuthGrantType[];
  name: string;
  redirectUris?: string[];
  responseTypes?: OAuthResponseType[];
  scopes?: string[];
  tokenEndpointAuthMethod: OAuthTokenEndpointAuthMethod;
}

export interface PatchOAuthClientRequest {
  attrs?: JsonValue;
  audiences?: string[] | null;
  description?: string | null;
  grantTypes?: OAuthGrantType[] | null;
  name?: string | null;
  redirectUris?: string[] | null;
  responseTypes?: OAuthResponseType[] | null;
  scopes?: string[] | null;
  tokenEndpointAuthMethod?: OAuthTokenEndpointAuthMethod | null;
}

export interface RotateOAuthClientSecretResponse extends OAuthClient {
  clientSecret: string;
}

export interface Organization {
  attrs?: JsonValue | null;
  createdAt: string;
  description?: string | null;
  id: string;
  name: string;
  updatedAt: string;
}

/** An organization a user belongs to, with the membership metadata. */
export interface UserOrganization extends Organization {
  role: string;
  memberSince?: string | null;
}

export interface OrganizationMembershipRequest {
  /** Free-form role; convention: owner | admin | member. Defaults to member. */
  role?: string;
}

export interface CreateOrganizationRequest {
  attrs?: JsonValue | null;
  description?: string | null;
  name: string;
}

export interface UpdateOrganizationRequest {
  attrs?: JsonValue | null;
  description?: string | null;
  name: string;
}

export interface ServiceAccount {
  createdAt: string;
  description?: string | null;
  id: string;
  name: string;
  orgId?: string | null;
  updatedAt: string;
}

export interface CreateServiceAccountRequest {
  description?: string | null;
  name: string;
  orgId?: string | null;
}

export interface UpdateServiceAccountRequest {
  description?: string | null;
  name: string;
}

export interface User {
  attrs: JsonValue;
  createdAt: string;
  email: string;
  familyName?: string | null;
  givenName?: string | null;
  id: string;
  nickname: string;
  phoneNumber?: string | null;
  picture?: string | null;
  updatedAt: string;
}

export interface CreateUserRequest {
  attrs?: JsonValue;
  email: string;
  familyName?: string | null;
  givenName?: string | null;
  nickname?: string | null;
  password: string;
  phoneNumber?: string | null;
  picture?: string | null;
}

export interface CreateUserInvitationRequest {
  attrs?: JsonValue;
  email: string;
  familyName?: string | null;
  givenName?: string | null;
  nickname?: string | null;
  phoneNumber?: string | null;
  picture?: string | null;
}

export interface UpdateUserRequest {
  email: string;
  familyName?: string | null;
  givenName?: string | null;
  nickname?: string | null;
  phoneNumber?: string | null;
  picture?: string | null;
}

export interface PatchUserRequest {
  email?: string | null;
  familyName?: string | null;
  givenName?: string | null;
  nickname?: string | null;
  phoneNumber?: string | null;
  picture?: string | null;
}

export interface UserAttrsRequest {
  attrs: JsonValue;
}

export type OutboxEventStatus = "pending" | "published";

export type OutboxEventPairRole = "target" | "control_plane";

export interface OutboxEvent {
  eventId: string;
  payload: JsonValue;
  seq: number;
  operationId: string | null;
  pairRole: string | null;
  status: OutboxEventStatus;
  createdAt: string;
  publishedAt: string | null;
}

export interface ListOutboxEventsQuery extends PaginationQuery {
  eventId?: string;
  operationId?: string;
  pairRole?: OutboxEventPairRole;
  status?: OutboxEventStatus;
}
