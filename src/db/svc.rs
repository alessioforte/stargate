use super::service;

use anyhow::Result;
use db::{
    DbStore,
    ent::{
        AdminKey, ApiKey, ApiKeyAuth, AuditContext, Credential, CredentialHistory, CredentialType,
        OAuthClient, OAuthConsent, OrgMember, OrgMembership, Organization, Profile, ServiceAccount,
        SuperAdmin, User,
    },
};

// ── Users ───────────────────────────────────────────────────────────────────

pub async fn create_user(
    profile: Profile,
    credential_type: CredentialType,
    value: &str,
    ctx: AuditContext,
) -> Result<User> {
    service()
        .create_user(profile, credential_type, value, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error creating user: {:?}", e))
}

pub async fn create_super_admin_user(
    profile: Profile,
    credential_type: CredentialType,
    value: &str,
    ctx: AuditContext,
) -> Result<User> {
    service()
        .create_super_admin_user(profile, credential_type, value, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error creating super admin user: {:?}", e))
}

pub async fn get_user_by_username(username: &str) -> Result<Option<User>> {
    let svc = service();
    svc.get_user_by_username(username).await
}

pub async fn get_user_by_id(id: &str) -> Result<Option<User>> {
    let svc = service();
    svc.get_user_by_id(id).await
}

pub async fn get_all_users(limit: i64, offset: i64) -> Result<Vec<User>> {
    let svc = service();
    svc.get_all_users(limit, offset).await
}

pub async fn count_users() -> Result<i64> {
    let svc = service();
    svc.count_users().await
}

pub async fn get_super_admin_users() -> Result<Vec<User>> {
    let svc = service();
    svc.get_super_admin_users().await
}

pub async fn get_super_admin_by_user_id(user_id: &str) -> Result<Option<SuperAdmin>> {
    let svc = service();
    svc.get_super_admin_by_user_id(user_id).await
}

pub async fn is_super_admin_user_id(user_id: &str) -> Result<bool> {
    Ok(get_super_admin_by_user_id(user_id).await?.is_some())
}

pub async fn super_admin_exists() -> Result<bool> {
    let svc = service();
    svc.super_admin_exists().await
}

pub async fn search_users(query: &str, limit: i64, offset: i64) -> Result<Vec<User>> {
    let svc = service();
    svc.search_users(query, limit, offset).await
}

pub async fn count_search_users(query: &str) -> Result<i64> {
    let svc = service();
    svc.count_search_users(query).await
}

pub async fn update_user(user: User, ctx: AuditContext) -> Result<User> {
    service()
        .update_user(user, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error updating user: {:?}", e))
}

pub async fn delete_user(id: &str, ctx: AuditContext) -> Result<Vec<String>> {
    service()
        .delete_user(id, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error deleting user: {:?}", e))
}

pub async fn change_password(
    user_id: &str,
    new_password: &str,
    ctx: AuditContext,
) -> Result<Credential> {
    service()
        .change_password(user_id, new_password, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error changing password: {:?}", e))
}

pub async fn get_credential(
    user_id: &str,
    credential_type: CredentialType,
) -> Result<Option<Credential>> {
    let svc = service();
    svc.get_credential(user_id, credential_type).await
}

pub async fn get_credential_history(user_id: &str, limit: i64) -> Result<Vec<CredentialHistory>> {
    service()
        .get_credential_history(user_id, limit)
        .await
        .inspect_err(|e| tracing::error!("Error fetching credential history: {:?}", e))
}

pub async fn rehash_credential(
    user_id: &str,
    old_value: &str,
    new_value: &str,
    ctx: AuditContext,
) -> Result<bool> {
    service()
        .rehash_credential(user_id, old_value, new_value, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error rehashing credential: {:?}", e))
}

// ── OAuth Clients ──────────────────────────────────────────────────────────

pub async fn create_oauth_client(client: OAuthClient, ctx: AuditContext) -> Result<OAuthClient> {
    service()
        .create_oauth_client(client, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error creating OAuth client: {:?}", e))
}

pub async fn get_oauth_client_by_client_id(client_id: &str) -> Result<Option<OAuthClient>> {
    let svc = service();
    svc.get_oauth_client_by_client_id(client_id).await
}

pub async fn get_all_oauth_clients(limit: i64, offset: i64) -> Result<Vec<OAuthClient>> {
    let svc = service();
    svc.get_all_oauth_clients(limit, offset).await
}

pub async fn count_oauth_clients() -> Result<i64> {
    let svc = service();
    svc.count_oauth_clients().await
}

pub async fn search_oauth_clients(
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<OAuthClient>> {
    let svc = service();
    svc.search_oauth_clients(query, limit, offset).await
}

pub async fn count_search_oauth_clients(query: &str) -> Result<i64> {
    let svc = service();
    svc.count_search_oauth_clients(query).await
}

pub async fn update_oauth_client(client: OAuthClient, ctx: AuditContext) -> Result<OAuthClient> {
    service()
        .update_oauth_client(client, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error updating OAuth client: {:?}", e))
}

pub async fn update_oauth_client_secret_hash(
    client_id: &str,
    client_secret_hash: Option<&str>,
    ctx: AuditContext,
) -> Result<OAuthClient> {
    service()
        .update_oauth_client_secret_hash(client_id, client_secret_hash, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error rotating OAuth client secret: {:?}", e))
}

pub async fn set_oauth_client_enabled(
    client_id: &str,
    enabled: bool,
    ctx: AuditContext,
) -> Result<OAuthClient> {
    service()
        .set_oauth_client_enabled(client_id, enabled, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error setting OAuth client enabled state: {:?}", e))
}

pub async fn delete_oauth_client(client_id: &str, ctx: AuditContext) -> Result<()> {
    service()
        .delete_oauth_client(client_id, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error deleting OAuth client: {:?}", e))
}

pub async fn get_active_oauth_consent(
    user_id: &str,
    client_id: &str,
) -> Result<Option<OAuthConsent>> {
    let svc = service();
    svc.get_active_oauth_consent(user_id, client_id).await
}

// ── Api Keys ────────────────────────────────────────────────────────────────

pub async fn create_user_api_key(
    user_id: &str,
    key_hash: &str,
    label: &str,
    attrs: Option<serde_json::Value>,
    org_id: Option<&str>,
    ctx: AuditContext,
) -> Result<ApiKey> {
    service()
        .create_user_api_key(user_id, key_hash, label, attrs, org_id, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error creating user API key: {:?}", e))
}

pub async fn create_service_account_api_key(
    service_account_id: &str,
    key_hash: &str,
    label: &str,
    attrs: Option<serde_json::Value>,
    ctx: AuditContext,
) -> Result<ApiKey> {
    service()
        .create_service_account_api_key(service_account_id, key_hash, label, attrs, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error creating service account API key: {:?}", e))
}

pub async fn get_api_key_by_hash(key_hash: &str) -> Result<Option<ApiKey>> {
    let svc = service();
    svc.get_api_key_by_hash(key_hash).await
}

pub async fn get_api_key_auth_by_hash(key_hash: &str) -> Result<Option<ApiKeyAuth>> {
    let svc = service();
    svc.get_api_key_auth_by_hash(key_hash).await
}

pub async fn get_api_key_by_id(id: &str) -> Result<Option<ApiKey>> {
    let svc = service();
    svc.get_api_key_by_id(id).await
}

pub async fn get_all_api_keys(limit: i64, offset: i64) -> Result<Vec<ApiKey>> {
    let svc = service();
    svc.get_all_api_keys(limit, offset).await
}

pub async fn get_api_keys_by_user_id(user_id: &str) -> Result<Vec<ApiKey>> {
    let svc = service();
    svc.get_api_keys_by_user_id(user_id).await
}

pub async fn get_all_user_api_keys(limit: i64, offset: i64) -> Result<Vec<ApiKey>> {
    let svc = service();
    svc.get_all_user_api_keys(limit, offset).await
}

pub async fn count_user_api_keys() -> Result<i64> {
    let svc = service();
    svc.count_user_api_keys().await
}

pub async fn get_api_keys_by_service_account_id(service_account_id: &str) -> Result<Vec<ApiKey>> {
    let svc = service();
    svc.get_api_keys_by_service_account_id(service_account_id)
        .await
}

pub async fn get_all_service_account_api_keys(limit: i64, offset: i64) -> Result<Vec<ApiKey>> {
    let svc = service();
    svc.get_all_service_account_api_keys(limit, offset).await
}

pub async fn count_service_account_api_keys() -> Result<i64> {
    let svc = service();
    svc.count_service_account_api_keys().await
}

pub async fn count_api_keys() -> Result<i64> {
    let svc = service();
    svc.count_api_keys().await
}

pub async fn search_api_keys(query: &str, limit: i64, offset: i64) -> Result<Vec<ApiKey>> {
    let svc = service();
    svc.search_api_keys(query, limit, offset).await
}

pub async fn count_search_api_keys(query: &str) -> Result<i64> {
    let svc = service();
    svc.count_search_api_keys(query).await
}

pub async fn update_api_key(api_key: ApiKey, ctx: AuditContext) -> Result<ApiKey> {
    service()
        .update_api_key(api_key, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error updating API key: {:?}", e))
}

pub async fn revoke_api_key(id: &str, ctx: AuditContext) -> Result<()> {
    service()
        .revoke_api_key(id, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error revoking API key: {:?}", e))
}

pub async fn delete_api_key(id: &str, ctx: AuditContext) -> Result<()> {
    service()
        .delete_api_key(id, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error deleting API key: {:?}", e))
}

// ── Admin Keys ──────────────────────────────────────────────────────────────

pub async fn create_admin_key(
    key_hash: &str,
    label: Option<String>,
    permissions: Vec<String>,
    ctx: AuditContext,
) -> Result<AdminKey> {
    service()
        .create_admin_key(key_hash, label, permissions, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error creating admin key: {:?}", e))
}

pub async fn get_admin_key_by_hash(key_hash: &str) -> Result<Option<AdminKey>> {
    let svc = service();
    svc.get_admin_key_by_hash(key_hash).await
}

pub async fn get_admin_key_by_id(id: &str) -> Result<Option<AdminKey>> {
    let svc = service();
    svc.get_admin_key_by_id(id).await
}

pub async fn get_all_admin_keys(limit: i64, offset: i64) -> Result<Vec<AdminKey>> {
    let svc = service();
    svc.get_all_admin_keys(limit, offset).await
}

pub async fn count_admin_keys() -> Result<i64> {
    let svc = service();
    svc.count_admin_keys().await
}

pub async fn update_admin_key(admin_key: AdminKey, ctx: AuditContext) -> Result<AdminKey> {
    service()
        .update_admin_key(admin_key, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error updating admin key: {:?}", e))
}

pub async fn revoke_admin_key(id: &str, ctx: AuditContext) -> Result<()> {
    service()
        .revoke_admin_key(id, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error revoking admin key: {:?}", e))
}

pub async fn delete_admin_key(id: &str, ctx: AuditContext) -> Result<()> {
    service()
        .delete_admin_key(id, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error deleting admin key: {:?}", e))
}

// ── Service Accounts ────────────────────────────────────────────────────────

pub async fn create_service_account(
    name: &str,
    description: Option<&str>,
    org_id: Option<&str>,
    ctx: AuditContext,
) -> Result<ServiceAccount> {
    service()
        .create_service_account(name, description, org_id, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error creating service account: {:?}", e))
}

pub async fn get_service_account_by_id(id: &str) -> Result<Option<ServiceAccount>> {
    let svc = service();
    svc.get_service_account_by_id(id).await
}

pub async fn get_all_service_accounts(limit: i64, offset: i64) -> Result<Vec<ServiceAccount>> {
    let svc = service();
    svc.get_all_service_accounts(limit, offset).await
}

pub async fn count_service_accounts() -> Result<i64> {
    let svc = service();
    svc.count_service_accounts().await
}

pub async fn search_service_accounts(
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<ServiceAccount>> {
    let svc = service();
    svc.search_service_accounts(query, limit, offset).await
}

pub async fn count_search_service_accounts(query: &str) -> Result<i64> {
    let svc = service();
    svc.count_search_service_accounts(query).await
}

pub async fn update_service_account(
    id: &str,
    name: &str,
    description: Option<&str>,
    ctx: AuditContext,
) -> Result<ServiceAccount> {
    service()
        .update_service_account(id, name, description, None, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error updating service account: {:?}", e))
}

pub async fn delete_service_account(id: &str, ctx: AuditContext) -> Result<Vec<String>> {
    service()
        .delete_service_account(id, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error deleting service account: {:?}", e))
}

// ── Organizations ───────────────────────────────────────────────────────────

pub async fn create_organization(
    name: &str,
    description: Option<&str>,
    attrs: Option<&serde_json::Value>,
    ctx: AuditContext,
) -> Result<Organization> {
    service()
        .create_organization(name, description, attrs, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error creating organization: {:?}", e))
}

pub async fn get_organization_by_id(id: &str) -> Result<Option<Organization>> {
    let svc = service();
    svc.get_organization_by_id(id).await
}

pub async fn get_all_organizations(limit: i64, offset: i64) -> Result<Vec<Organization>> {
    let svc = service();
    svc.get_all_organizations(limit, offset).await
}

pub async fn count_organizations() -> Result<i64> {
    let svc = service();
    svc.count_organizations().await
}

pub async fn search_organizations(
    query: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<Organization>> {
    let svc = service();
    svc.search_organizations(query, limit, offset).await
}

pub async fn count_search_organizations(query: &str) -> Result<i64> {
    let svc = service();
    svc.count_search_organizations(query).await
}

pub async fn update_organization(
    id: &str,
    name: &str,
    description: Option<&str>,
    attrs: Option<&serde_json::Value>,
    ctx: AuditContext,
) -> Result<Organization> {
    service()
        .update_organization(id, name, description, attrs, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error updating organization: {:?}", e))
}

pub async fn delete_organization(id: &str, ctx: AuditContext) -> Result<Vec<String>> {
    service()
        .delete_organization(id, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error deleting organization: {:?}", e))
}

pub async fn add_user_to_organization(
    user_id: &str,
    org_id: &str,
    role: &str,
    ctx: AuditContext,
) -> Result<()> {
    service()
        .add_user_to_organization(user_id, org_id, role, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error adding user to organization: {:?}", e))
}

pub async fn remove_user_from_organization(
    user_id: &str,
    org_id: &str,
    ctx: AuditContext,
) -> Result<Vec<String>> {
    service()
        .remove_user_from_organization(user_id, org_id, ctx)
        .await
        .inspect_err(|e| tracing::error!("Error removing user from organization: {:?}", e))
}

pub async fn get_organization_users(org_id: &str) -> Result<Vec<OrgMember>> {
    let svc = service();
    svc.get_organization_users(org_id).await
}

pub async fn get_organization_users_paginated(
    org_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<OrgMember>> {
    let svc = service();
    svc.get_organization_users_paginated(org_id, limit, offset)
        .await
}

pub async fn count_organization_users(org_id: &str) -> Result<i64> {
    let svc = service();
    svc.count_organization_users(org_id).await
}

pub async fn get_user_organizations(user_id: &str) -> Result<Vec<OrgMembership>> {
    let svc = service();
    svc.get_user_organizations(user_id).await
}

pub async fn get_user_organization(user_id: &str, org_id: &str) -> Result<Option<OrgMembership>> {
    let svc = service();
    svc.get_user_organization(user_id, org_id).await
}
