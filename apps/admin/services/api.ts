import type { AxiosRequestConfig } from "axios";
import { Http, type Response as HttpResponse } from "./http";
import type {
  AccessControlRulesResponse,
  AdminHealth,
  AdminKey,
  ApiKey,
  ApiKeyAttrsRequest,
  ApiKeyQuery,
  Configuration,
  ConfigurationsQuery,
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
  List,
  MessageResponse,
  OAuthClient,
  Organization,
  PatchOAuthClientRequest,
  PatchUserRequest,
  Query,
  RotateOAuthClientSecretResponse,
  ServiceAccount,
  UpdateAccessControlRulesRequest,
  UpdateAdminKeyPermissionsRequest,
  UpdateOAuthClientRequest,
  OrganizationMembershipRequest,
  UpdateOrganizationRequest,
  UpdateServiceAccountRequest,
  UpdateUserRequest,
  User,
  UserAttrsRequest,
  UserOrganization,
  ValidateAccessControlRulesRequest,
  ValidateAccessControlRulesResponse,
} from "./types";

export default class AdminApiService {
  private readonly http: Http;
  private readonly basePath: string;

  constructor(http: Http, basePath = "/admin") {
    this.http = http;
    this.basePath = basePath.replace(/\/+$/, "");
  }

  private request<T>(
    config: AxiosRequestConfig,
  ): Promise<HttpResponse<T | null>> {
    return this.http.authRequest<T>(config);
  }

  private url(path: string) {
    return `${this.basePath}${path}`;
  }

  private pathParam(value: string) {
    return encodeURIComponent(value);
  }

  async getAdminHealth() {
    return this.request<AdminHealth>({
      url: this.url("/health"),
      method: "GET",
    });
  }

  async getConfigurations(query?: ConfigurationsQuery) {
    return this.request<Configuration | string>({
      url: this.url("/configurations"),
      method: "GET",
      params: query,
    });
  }

  async updateConfigurations(configuration: Configuration) {
    return this.request<void>({
      url: this.url("/configurations"),
      method: "PUT",
      data: configuration,
    });
  }

  async getAccessControlRules() {
    return this.request<AccessControlRulesResponse>({
      url: this.url("/access-control/rules"),
      method: "GET",
    });
  }

  async updateAccessControlRules(body: UpdateAccessControlRulesRequest) {
    return this.request<AccessControlRulesResponse>({
      url: this.url("/access-control/rules"),
      method: "PUT",
      data: body,
    });
  }

  async validateAccessControlRules(body: ValidateAccessControlRulesRequest) {
    return this.request<ValidateAccessControlRulesResponse>({
      url: this.url("/access-control/rules/validate"),
      method: "POST",
      data: body,
    });
  }

  async evaluateAccessControlRules(body: EvaluateAccessControlRequest) {
    return this.request<EvaluateAccessControlResponse>({
      url: this.url("/access-control/rules/evaluate"),
      method: "POST",
      data: body,
    });
  }

  async getAdminKeys(query?: Query) {
    return this.request<List<AdminKey>>({
      url: this.url("/admin-keys"),
      method: "GET",
      params: query,
    });
  }

  async createAdminKey(body: CreateAdminKeyRequest) {
    return this.request<CreateAdminKeyResponse>({
      url: this.url("/admin-keys"),
      method: "POST",
      data: body,
    });
  }

  async getAdminKey(id: string) {
    return this.request<AdminKey>({
      url: this.url(`/admin-keys/${this.pathParam(id)}`),
      method: "GET",
    });
  }

  async deleteAdminKey(id: string) {
    return this.request<MessageResponse>({
      url: this.url(`/admin-keys/${this.pathParam(id)}`),
      method: "DELETE",
    });
  }

  async updateAdminKeyPermissions(
    id: string,
    body: UpdateAdminKeyPermissionsRequest,
  ) {
    return this.request<AdminKey>({
      url: this.url(`/admin-keys/${this.pathParam(id)}/permissions`),
      method: "PUT",
      data: body,
    });
  }

  async revokeAdminKey(id: string) {
    return this.request<MessageResponse>({
      url: this.url(`/admin-keys/${this.pathParam(id)}/revoke`),
      method: "PUT",
    });
  }

  async getApiKeys(query?: ApiKeyQuery) {
    return this.request<List<ApiKey>>({
      url: this.url("/api-keys"),
      method: "GET",
      params: query,
    });
  }

  async createApiKey(body: CreateApiKeyRequest) {
    return this.request<CreateApiKeyResponse>({
      url: this.url("/api-keys"),
      method: "POST",
      data: body,
    });
  }

  async getApiKey(id: string) {
    return this.request<ApiKey>({
      url: this.url(`/api-keys/${this.pathParam(id)}`),
      method: "GET",
    });
  }

  async deleteApiKey(id: string) {
    return this.request<MessageResponse>({
      url: this.url(`/api-keys/${this.pathParam(id)}`),
      method: "DELETE",
    });
  }

  async updateApiKeyAttrs(id: string, body: ApiKeyAttrsRequest) {
    return this.request<ApiKey>({
      url: this.url(`/api-keys/${this.pathParam(id)}/attrs`),
      method: "PUT",
      data: body,
    });
  }

  async patchApiKeyAttrs(id: string, body: ApiKeyAttrsRequest) {
    return this.request<ApiKey>({
      url: this.url(`/api-keys/${this.pathParam(id)}/attrs`),
      method: "PATCH",
      data: body,
    });
  }

  async revokeApiKey(id: string) {
    return this.request<MessageResponse>({
      url: this.url(`/api-keys/${this.pathParam(id)}/revoke`),
      method: "PUT",
    });
  }

  async getOAuthClients(query?: Query) {
    return this.request<List<OAuthClient>>({
      url: this.url("/oauth/clients"),
      method: "GET",
      params: query,
    });
  }

  async createOAuthClient(body: CreateOAuthClientRequest) {
    return this.request<CreateOAuthClientResponse>({
      url: this.url("/oauth/clients"),
      method: "POST",
      data: body,
    });
  }

  async getOAuthClient(clientId: string) {
    return this.request<OAuthClient>({
      url: this.url(`/oauth/clients/${this.pathParam(clientId)}`),
      method: "GET",
    });
  }

  async updateOAuthClient(clientId: string, body: UpdateOAuthClientRequest) {
    return this.request<OAuthClient>({
      url: this.url(`/oauth/clients/${this.pathParam(clientId)}`),
      method: "PUT",
      data: body,
    });
  }

  async patchOAuthClient(clientId: string, body: PatchOAuthClientRequest) {
    return this.request<OAuthClient>({
      url: this.url(`/oauth/clients/${this.pathParam(clientId)}`),
      method: "PATCH",
      data: body,
    });
  }

  async deleteOAuthClient(clientId: string) {
    return this.request<MessageResponse>({
      url: this.url(`/oauth/clients/${this.pathParam(clientId)}`),
      method: "DELETE",
    });
  }

  async disableOAuthClient(clientId: string) {
    return this.request<OAuthClient>({
      url: this.url(`/oauth/clients/${this.pathParam(clientId)}/disable`),
      method: "PUT",
    });
  }

  async enableOAuthClient(clientId: string) {
    return this.request<OAuthClient>({
      url: this.url(`/oauth/clients/${this.pathParam(clientId)}/enable`),
      method: "PUT",
    });
  }

  async rotateOAuthClientSecret(clientId: string) {
    return this.request<RotateOAuthClientSecretResponse>({
      url: this.url(`/oauth/clients/${this.pathParam(clientId)}/rotate-secret`),
      method: "PUT",
    });
  }

  async getOrganizations(query?: Query) {
    return this.request<List<Organization>>({
      url: this.url("/organizations"),
      method: "GET",
      params: query,
    });
  }

  async createOrganization(body: CreateOrganizationRequest) {
    return this.request<Organization>({
      url: this.url("/organizations"),
      method: "POST",
      data: body,
    });
  }

  async getOrganization(id: string) {
    return this.request<Organization>({
      url: this.url(`/organizations/${this.pathParam(id)}`),
      method: "GET",
    });
  }

  async updateOrganization(id: string, body: UpdateOrganizationRequest) {
    return this.request<Organization>({
      url: this.url(`/organizations/${this.pathParam(id)}`),
      method: "PUT",
      data: body,
    });
  }

  async deleteOrganization(id: string) {
    return this.request<void>({
      url: this.url(`/organizations/${this.pathParam(id)}`),
      method: "DELETE",
    });
  }

  async getServiceAccounts(query?: Query) {
    return this.request<List<ServiceAccount>>({
      url: this.url("/service-accounts"),
      method: "GET",
      params: query,
    });
  }

  async createServiceAccount(body: CreateServiceAccountRequest) {
    return this.request<ServiceAccount>({
      url: this.url("/service-accounts"),
      method: "POST",
      data: body,
    });
  }

  async getServiceAccount(id: string) {
    return this.request<ServiceAccount>({
      url: this.url(`/service-accounts/${this.pathParam(id)}`),
      method: "GET",
    });
  }

  async updateServiceAccount(id: string, body: UpdateServiceAccountRequest) {
    return this.request<ServiceAccount>({
      url: this.url(`/service-accounts/${this.pathParam(id)}`),
      method: "PUT",
      data: body,
    });
  }

  async deleteServiceAccount(id: string) {
    return this.request<MessageResponse>({
      url: this.url(`/service-accounts/${this.pathParam(id)}`),
      method: "DELETE",
    });
  }

  async getUsers(query?: Query) {
    return this.request<List<User>>({
      url: this.url("/users"),
      method: "GET",
      params: query,
    });
  }

  async getSuperAdminUsers() {
    return this.request<List<User>>({
      url: this.url("/users/super-admins"),
      method: "GET",
    });
  }

  async createUser(body: CreateUserRequest) {
    return this.request<User>({
      url: this.url("/users"),
      method: "POST",
      data: body,
    });
  }

  async inviteUser(body: CreateUserInvitationRequest) {
    return this.request<MessageResponse>({
      url: this.url("/users/invitations"),
      method: "POST",
      data: body,
    });
  }

  async getOrganizationUsers(orgId: string, query?: Query) {
    return this.request<List<User>>({
      url: this.url(`/users/organizations/${this.pathParam(orgId)}`),
      method: "GET",
      params: query,
    });
  }

  async getUser(id: string) {
    return this.request<User>({
      url: this.url(`/users/${this.pathParam(id)}`),
      method: "GET",
    });
  }

  async updateUser(id: string, body: UpdateUserRequest) {
    return this.request<User>({
      url: this.url(`/users/${this.pathParam(id)}`),
      method: "PUT",
      data: body,
    });
  }

  async patchUser(id: string, body: PatchUserRequest) {
    return this.request<User>({
      url: this.url(`/users/${this.pathParam(id)}`),
      method: "PATCH",
      data: body,
    });
  }

  async deleteUser(id: string) {
    return this.request<MessageResponse>({
      url: this.url(`/users/${this.pathParam(id)}`),
      method: "DELETE",
    });
  }

  async updateUserAttrs(id: string, body: UserAttrsRequest) {
    return this.request<User>({
      url: this.url(`/users/${this.pathParam(id)}/attrs`),
      method: "PUT",
      data: body,
    });
  }

  async patchUserAttrs(id: string, body: UserAttrsRequest) {
    return this.request<User>({
      url: this.url(`/users/${this.pathParam(id)}/attrs`),
      method: "PATCH",
      data: body,
    });
  }

  async getUserOrganizations(id: string) {
    return this.request<UserOrganization[]>({
      url: this.url(`/users/${this.pathParam(id)}/organizations`),
      method: "GET",
    });
  }

  /** Upsert: adds the membership or updates the role of an existing one. */
  async addUserToOrganization(
    id: string,
    orgId: string,
    body?: OrganizationMembershipRequest,
  ) {
    return this.request<MessageResponse>({
      url: this.url(
        `/users/${this.pathParam(id)}/organizations/${this.pathParam(orgId)}`,
      ),
      method: "PUT",
      data: body,
    });
  }

  async removeUserFromOrganization(id: string, orgId: string) {
    return this.request<MessageResponse>({
      url: this.url(
        `/users/${this.pathParam(id)}/organizations/${this.pathParam(orgId)}`,
      ),
      method: "DELETE",
    });
  }
}
