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

export interface AdminKey {
  id: string;
  label?: string | null;
  permissions: string[];
  revoked: boolean;
}

export interface CreateAdminKeyRequest {
  label?: string | null;
  permissions: string[];
}

export interface CreateAdminKeyResponse extends AdminKey {
  apiKey: string;
}

export interface UpdateAdminKeyPermissionsRequest {
  permissions: string[];
}

export type ApiKeyOwnerType = "user" | "service_account";

export interface ApiKeyQuery extends PaginationQuery {
  ownerType?: ApiKeyOwnerType;
  serviceAccountId?: string;
  userId?: string;
}

export interface ApiKey {
  attrs: JsonValue;
  id: string;
  label: string;
  revoked: boolean;
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

export interface OAuthClient {
  attrs: JsonValue;
  audiences: string[];
  clientId: string;
  createdAt: string;
  description?: string | null;
  enabled: boolean;
  grantTypes: string[];
  name: string;
  orgId?: string | null;
  redirectUris: string[];
  responseTypes: string[];
  scopes: string[];
  serviceAccountId?: string | null;
  tokenEndpointAuthMethod: string;
  updatedAt: string;
}

export interface CreateOAuthClientRequest {
  attrs?: JsonValue;
  audiences?: string[] | null;
  clientId?: string | null;
  description?: string | null;
  grantTypes: string[];
  name: string;
  orgId?: string | null;
  redirectUris?: string[] | null;
  responseTypes?: string[] | null;
  scopes?: string[] | null;
  serviceAccountId?: string | null;
  tokenEndpointAuthMethod?: string | null;
}

export interface CreateOAuthClientResponse extends OAuthClient {
  clientSecret?: string | null;
}

export interface UpdateOAuthClientRequest {
  attrs?: JsonValue;
  audiences?: string[];
  description?: string | null;
  grantTypes: string[];
  name: string;
  orgId?: string | null;
  redirectUris?: string[];
  responseTypes?: string[];
  scopes?: string[];
  serviceAccountId?: string | null;
  tokenEndpointAuthMethod: string;
}

export interface PatchOAuthClientRequest {
  attrs?: JsonValue;
  audiences?: string[] | null;
  description?: string | null;
  grantTypes?: string[] | null;
  name?: string | null;
  orgId?: string | null;
  redirectUris?: string[] | null;
  responseTypes?: string[] | null;
  scopes?: string[] | null;
  serviceAccountId?: string | null;
  tokenEndpointAuthMethod?: string | null;
}

export interface RotateOAuthClientSecretResponse extends OAuthClient {
  clientSecret: string;
}

export interface Organization {
  attrs?: JsonValue | null;
  description?: string | null;
  id: string;
  name: string;
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
  description?: string | null;
  id: string;
  name: string;
  orgId?: string | null;
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
  email: string;
  familyName?: string | null;
  givenName?: string | null;
  id: string;
  nickname: string;
  phoneNumber?: string | null;
  picture?: string | null;
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
