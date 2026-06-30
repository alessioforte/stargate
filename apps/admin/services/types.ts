export type JsonPrimitive = string | number | boolean | null;
export type JsonValue =
  | JsonPrimitive
  | JsonValue[]
  | { [key: string]: JsonValue };

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
}

export interface ComponentStatus {
  detail?: string | null;
  status: string;
}

export interface AdminHealth {
  database: ComponentStatus;
  name: string;
  redis: ComponentStatus;
  status: string;
  version: string;
}

export interface ConfigurationsQuery {
  format?: string;
}

export type Configuration = JsonValue;

export const AdminKeyPermission = {
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
  orgId?: string | null;
  redirectUris: string[];
  responseTypes: OAuthResponseType[];
  scopes: string[];
  serviceAccountId?: string | null;
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
  orgId?: string | null;
  redirectUris?: string[] | null;
  responseTypes?: OAuthResponseType[] | null;
  scopes?: string[] | null;
  serviceAccountId?: string | null;
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
  orgId?: string | null;
  redirectUris?: string[];
  responseTypes?: OAuthResponseType[];
  scopes?: string[];
  serviceAccountId?: string | null;
  tokenEndpointAuthMethod: OAuthTokenEndpointAuthMethod;
}

export interface PatchOAuthClientRequest {
  attrs?: JsonValue;
  audiences?: string[] | null;
  description?: string | null;
  grantTypes?: OAuthGrantType[] | null;
  name?: string | null;
  orgId?: string | null;
  redirectUris?: string[] | null;
  responseTypes?: OAuthResponseType[] | null;
  scopes?: string[] | null;
  serviceAccountId?: string | null;
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
