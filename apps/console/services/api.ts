import type { AxiosRequestConfig } from "axios";
import type { Http, Response as HttpResponse } from "./http";
import type {
  AccessControlRulesResponse,
  ApiMessageCatalog,
  AdminHealth,
  AdminMe,
  AdminOverview,
  AdminSession,
  AdminKey,
  ApiKey,
  ApiKeyAttrsRequest,
  ApiKeyQuery,
  Configuration,
  CreateAdminKeyRequest,
  CreateAdminKeyResponse,
  CreateApiKeyRequest,
  CreateApiKeyResponse,
  CreateOAuthClientRequest,
  CreateOAuthClientResponse,
  CreateOrganizationRequest,
  CreateServiceAccountRequest,
  CreateUserRequest,
  CreateUserInvitationRequest,
  EvaluateAccessControlRequest,
  EvaluateAccessControlResponse,
  EvaluateAccessControlCapabilitiesRequest,
  EvaluateAccessControlCapabilitiesResponse,
  List,
  ListAdminSessionsQuery,
  ListOutboxEventsQuery,
  MessageResponse,
  OAuthClient,
  Organization,
  OutboxEvent,
  Query,
  RotateOAuthClientSecretResponse,
  SessionRevocationResponse,
  ServiceAccount,
  UpdateAccessControlRulesRequest,
  UpdateAdminKeyPermissionsRequest,
  UpdateOAuthClientRequest,
  OrganizationMembershipRequest,
  UpdateOrganizationRequest,
  UpdateServiceAccountRequest,
  UpdateUserRequest,
  User,
  UserSessionsRevocationResponse,
  UserAttrsRequest,
  UserOrganization,
  ValidateAccessControlRulesRequest,
  ValidateAccessControlRulesResponse,
} from "./types";

export default class AdminApiService {
  private readonly http: Http;

  constructor(http: Http) {
    this.http = http;
  }

  private request<T>(config: AxiosRequestConfig): Promise<HttpResponse<T>> {
    return this.http.authRequest<T>({ ...config, url: `/admin${config.url}` });
  }

  getApiMessages(locale: string) {
    return this.http.request<ApiMessageCatalog>({
      url: `/i18n/${encodeURIComponent(locale)}`,
      method: "GET",
    });
  }

  getAdminHealth() {
    return this.request<AdminHealth>({
      url: "/health",
      method: "GET",
      validateStatus: (status) => status === 200 || status === 503,
    });
  }

  getAdminMe() {
    return this.request<AdminMe>({
      url: "/me",
      method: "GET",
    });
  }

  getAdminOverview() {
    return this.request<AdminOverview>({
      url: "/overview",
      method: "GET",
    });
  }

  getAdminSessions(query?: ListAdminSessionsQuery) {
    return this.request<List<AdminSession>>({
      url: "/sessions",
      method: "GET",
      params: query,
    });
  }

  revokeAdminSession(sessionId: string) {
    return this.request<SessionRevocationResponse>({
      url: `/sessions/${encodeURIComponent(sessionId)}`,
      method: "DELETE",
    });
  }

  revokeUserSessions(userId: string) {
    return this.request<UserSessionsRevocationResponse>({
      url: `/users/${encodeURIComponent(userId)}/sessions`,
      method: "DELETE",
    });
  }

  getConfigurations() {
    return this.request<Configuration>({
      url: "/configurations",
      method: "GET",
    });
  }

  updateConfigurations(configuration: Configuration) {
    return this.request<void>({
      url: "/configurations",
      method: "PUT",
      data: configuration,
    });
  }

  getAccessControlRules() {
    return this.request<AccessControlRulesResponse>({
      url: "/access-control/rules",
      method: "GET",
    });
  }

  updateAccessControlRules(body: UpdateAccessControlRulesRequest) {
    return this.request<AccessControlRulesResponse>({
      url: "/access-control/rules",
      method: "PUT",
      data: body,
    });
  }

  validateAccessControlRules(body: ValidateAccessControlRulesRequest) {
    return this.request<ValidateAccessControlRulesResponse>({
      url: "/access-control/rules/validate",
      method: "POST",
      data: body,
    });
  }

  evaluateAccessControlRules(body: EvaluateAccessControlRequest) {
    return this.request<EvaluateAccessControlResponse>({
      url: "/access-control/rules/evaluate",
      method: "POST",
      data: body,
    });
  }

  evaluateAccessControlCapabilities(
    body: EvaluateAccessControlCapabilitiesRequest,
  ) {
    return this.request<EvaluateAccessControlCapabilitiesResponse>({
      url: "/access-control/capabilities/evaluate",
      method: "POST",
      data: body,
    });
  }

  getAdminKeys(query?: Query) {
    return this.request<List<AdminKey>>({
      url: "/admin-keys",
      method: "GET",
      params: query,
    });
  }

  createAdminKey(body: CreateAdminKeyRequest) {
    return this.request<CreateAdminKeyResponse>({
      url: "/admin-keys",
      method: "POST",
      data: body,
    });
  }

  updateAdminKeyPermissions(
    id: string,
    body: UpdateAdminKeyPermissionsRequest,
  ) {
    return this.request<AdminKey>({
      url: `/admin-keys/${encodeURIComponent(id)}/permissions`,
      method: "PUT",
      data: body,
    });
  }

  revokeAdminKey(id: string) {
    return this.request<MessageResponse>({
      url: `/admin-keys/${encodeURIComponent(id)}/revoke`,
      method: "PUT",
    });
  }

  getApiKeys(query?: ApiKeyQuery) {
    return this.request<List<ApiKey>>({
      url: "/api-keys",
      method: "GET",
      params: query,
    });
  }

  createApiKey(body: CreateApiKeyRequest) {
    return this.request<CreateApiKeyResponse>({
      url: "/api-keys",
      method: "POST",
      data: body,
    });
  }

  deleteApiKey(id: string) {
    return this.request<MessageResponse>({
      url: `/api-keys/${encodeURIComponent(id)}`,
      method: "DELETE",
    });
  }

  updateApiKeyAttrs(id: string, body: ApiKeyAttrsRequest) {
    return this.request<ApiKey>({
      url: `/api-keys/${encodeURIComponent(id)}/attrs`,
      method: "PUT",
      data: body,
    });
  }

  revokeApiKey(id: string) {
    return this.request<MessageResponse>({
      url: `/api-keys/${encodeURIComponent(id)}/revoke`,
      method: "PUT",
    });
  }

  getOAuthClients(query?: Query) {
    return this.request<List<OAuthClient>>({
      url: "/oauth/clients",
      method: "GET",
      params: query,
    });
  }

  createOAuthClient(body: CreateOAuthClientRequest) {
    return this.request<CreateOAuthClientResponse>({
      url: "/oauth/clients",
      method: "POST",
      data: body,
    });
  }

  updateOAuthClient(clientId: string, body: UpdateOAuthClientRequest) {
    return this.request<OAuthClient>({
      url: `/oauth/clients/${encodeURIComponent(clientId)}`,
      method: "PUT",
      data: body,
    });
  }

  deleteOAuthClient(clientId: string) {
    return this.request<MessageResponse>({
      url: `/oauth/clients/${encodeURIComponent(clientId)}`,
      method: "DELETE",
    });
  }

  disableOAuthClient(clientId: string) {
    return this.request<OAuthClient>({
      url: `/oauth/clients/${encodeURIComponent(clientId)}/disable`,
      method: "PUT",
    });
  }

  enableOAuthClient(clientId: string) {
    return this.request<OAuthClient>({
      url: `/oauth/clients/${encodeURIComponent(clientId)}/enable`,
      method: "PUT",
    });
  }

  rotateOAuthClientSecret(clientId: string) {
    return this.request<RotateOAuthClientSecretResponse>({
      url: `/oauth/clients/${encodeURIComponent(clientId)}/rotate-secret`,
      method: "PUT",
    });
  }

  getOrganizations(query?: Query) {
    return this.request<List<Organization>>({
      url: "/organizations",
      method: "GET",
      params: query,
    });
  }

  createOrganization(body: CreateOrganizationRequest) {
    return this.request<Organization>({
      url: "/organizations",
      method: "POST",
      data: body,
    });
  }

  updateOrganization(id: string, body: UpdateOrganizationRequest) {
    return this.request<Organization>({
      url: `/organizations/${encodeURIComponent(id)}`,
      method: "PUT",
      data: body,
    });
  }

  getServiceAccounts(query?: Query) {
    return this.request<List<ServiceAccount>>({
      url: "/service-accounts",
      method: "GET",
      params: query,
    });
  }

  createServiceAccount(body: CreateServiceAccountRequest) {
    return this.request<ServiceAccount>({
      url: "/service-accounts",
      method: "POST",
      data: body,
    });
  }

  updateServiceAccount(id: string, body: UpdateServiceAccountRequest) {
    return this.request<ServiceAccount>({
      url: `/service-accounts/${encodeURIComponent(id)}`,
      method: "PUT",
      data: body,
    });
  }

  deleteServiceAccount(id: string) {
    return this.request<MessageResponse>({
      url: `/service-accounts/${encodeURIComponent(id)}`,
      method: "DELETE",
    });
  }

  getUsers(query?: Query) {
    return this.request<List<User>>({
      url: "/users",
      method: "GET",
      params: query,
    });
  }

  getSuperAdminUsers() {
    return this.request<List<User>>({
      url: "/users/super-admins",
      method: "GET",
    });
  }

  createUser(body: CreateUserRequest) {
    return this.request<User>({
      url: "/users",
      method: "POST",
      data: body,
    });
  }

  inviteUser(body: CreateUserInvitationRequest) {
    return this.request<MessageResponse>({
      url: "/users/invitations",
      method: "POST",
      data: body,
    });
  }

  updateUser(id: string, body: UpdateUserRequest) {
    return this.request<User>({
      url: `/users/${encodeURIComponent(id)}`,
      method: "PUT",
      data: body,
    });
  }

  deleteUser(id: string) {
    return this.request<MessageResponse>({
      url: `/users/${encodeURIComponent(id)}`,
      method: "DELETE",
    });
  }

  updateUserAttrs(id: string, body: UserAttrsRequest) {
    return this.request<User>({
      url: `/users/${encodeURIComponent(id)}/attrs`,
      method: "PUT",
      data: body,
    });
  }

  getUserOrganizations(id: string) {
    return this.request<UserOrganization[]>({
      url: `/users/${encodeURIComponent(id)}/organizations`,
      method: "GET",
    });
  }

  /** Upsert: adds the membership or updates the role of an existing one. */
  addUserToOrganization(
    id: string,
    orgId: string,
    body?: OrganizationMembershipRequest,
  ) {
    return this.request<MessageResponse>({
      url: `/users/${encodeURIComponent(id)}/organizations/${encodeURIComponent(orgId)}`,
      method: "PUT",
      data: body,
    });
  }

  removeUserFromOrganization(id: string, orgId: string) {
    return this.request<MessageResponse>({
      url: `/users/${encodeURIComponent(id)}/organizations/${encodeURIComponent(orgId)}`,
      method: "DELETE",
    });
  }

  getOutboxEvents(query?: ListOutboxEventsQuery) {
    return this.request<List<OutboxEvent>>({
      url: "/outbox-events",
      method: "GET",
      params: query,
    });
  }

  getOutboxEvent(eventId: string) {
    return this.request<OutboxEvent>({
      url: `/outbox-events/${encodeURIComponent(eventId)}`,
      method: "GET",
    });
  }
}
