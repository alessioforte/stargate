use super::service;
use crate::aud::audit;

use anyhow::Result;
use db::{
    Transaction,
    ent::{
        ActionType, AdminKey, ApiKey, AuditContext, Credential, CredentialType, Organization,
        Profile, ServiceAccount, User,
    },
    repo::ADMIN_KEY,
    repo::API_KEY,
    repo::CREDENTIAL,
    repo::ORGANIZATION,
    repo::SERVICE_ACCOUNT,
    repo::USER,
};

// ── Users ───────────────────────────────────────────────────────────────────

pub async fn create_user(
    profile: Profile,
    credential_type: CredentialType,
    value: &str,
    ctx: AuditContext,
) -> Result<User> {
    let svc = service();
    match svc.create_user(profile, credential_type, value).await {
        Ok(user) => {
            let metadata = serde_json::to_value(&user).unwrap_or_default();
            let resource = USER.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(user.id.clone())
                .with_metadata(metadata);
            audit::creation!(ctx);
            Ok(user)
        }
        Err(e) => {
            tracing::error!("Error creating user: {:?}", e);
            Err(e)
        }
    }
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

pub async fn search_users(query: &str, limit: i64, offset: i64) -> Result<Vec<User>> {
    let svc = service();
    svc.search_users(query, limit, offset).await
}

pub async fn count_search_users(query: &str) -> Result<i64> {
    let svc = service();
    svc.count_search_users(query).await
}

pub async fn update_user(user: User, ctx: AuditContext) -> Result<User> {
    let svc = service();
    match svc.update_user(user).await {
        Ok(updated_user) => {
            let metadata = serde_json::to_value(&updated_user).unwrap_or_default();
            let resource = USER.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(updated_user.id.clone())
                .with_metadata(metadata);
            audit::modification!(ctx);
            Ok(updated_user)
        }
        Err(e) => {
            tracing::error!("Error updating user: {:?}", e);
            Err(e)
        }
    }
}

pub async fn delete_user(id: &str, ctx: AuditContext) -> Result<()> {
    let svc = service();
    match svc.delete_user(id).await {
        Ok(()) => {
            let resource = USER.to_string();
            let ctx = ctx.with_resource(resource).with_resource_id(id.to_string());
            audit::deletion!(ctx);
            Ok(())
        }
        Err(e) => {
            tracing::error!("Error deleting user: {:?}", e);
            Err(e)
        }
    }
}

pub async fn change_password(
    user_id: &str,
    new_password: &str,
    ctx: AuditContext,
) -> Result<Credential> {
    let svc = service();
    match svc.change_password(user_id, new_password).await {
        Ok(credential) => {
            let resource = CREDENTIAL.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(user_id.to_string());
            audit::modification!(ctx);
            Ok(credential)
        }
        Err(e) => {
            tracing::error!("Error changing password: {:?}", e);
            Err(e)
        }
    }
}

pub async fn get_credential(
    user_id: &str,
    credential_type: CredentialType,
) -> Result<Option<Credential>> {
    let svc = service();
    svc.get_credential(user_id, credential_type).await
}

// ── Api Keys ────────────────────────────────────────────────────────────────

pub async fn create_user_api_key(
    user_id: &str,
    key_hash: &str,
    label: &str,
    attrs: Option<serde_json::Value>,
    ctx: AuditContext,
) -> Result<ApiKey> {
    let svc = service();
    match svc
        .create_user_api_key(user_id, key_hash, label, attrs)
        .await
    {
        Ok(api_key) => {
            let resource = API_KEY.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(api_key.id.to_string());
            audit::creation!(ctx);
            Ok(api_key)
        }
        Err(e) => {
            tracing::error!("Error creating user API key: {:?}", e);
            Err(e)
        }
    }
}

pub async fn create_service_account_api_key(
    service_account_id: &str,
    key_hash: &str,
    label: &str,
    attrs: Option<serde_json::Value>,
    ctx: AuditContext,
) -> Result<ApiKey> {
    let svc = service();
    match svc
        .create_service_account_api_key(service_account_id, key_hash, label, attrs)
        .await
    {
        Ok(api_key) => {
            let resource = API_KEY.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(api_key.id.to_string());
            audit::creation!(ctx);
            Ok(api_key)
        }
        Err(e) => {
            tracing::error!("Error creating service account API key: {:?}", e);
            Err(e)
        }
    }
}

pub async fn get_api_key_by_hash(key_hash: &str) -> Result<Option<ApiKey>> {
    let svc = service();
    svc.get_api_key_by_hash(key_hash).await
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

pub async fn update_api_key(api_key: ApiKey, ctx: AuditContext) -> Result<ApiKey> {
    let svc = service();
    match svc.update_api_key(api_key).await {
        Ok(updated) => {
            let resource = API_KEY.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(updated.id.clone());
            audit::modification!(ctx);
            Ok(updated)
        }
        Err(e) => {
            tracing::error!("Error updating API key: {:?}", e);
            Err(e)
        }
    }
}

pub async fn revoke_api_key(id: &str, ctx: AuditContext) -> Result<()> {
    let svc = service();
    match svc.revoke_api_key(id).await {
        Ok(()) => {
            let resource = API_KEY.to_string();
            let ctx = ctx.with_resource(resource).with_resource_id(id.to_string());
            audit::modification!(ctx);
            Ok(())
        }
        Err(e) => {
            tracing::error!("Error revoking API key: {:?}", e);
            Err(e)
        }
    }
}

pub async fn delete_api_key(id: &str, ctx: AuditContext) -> Result<()> {
    let svc = service();
    match svc.delete_api_key(id).await {
        Ok(()) => {
            let resource = API_KEY.to_string();
            let ctx = ctx.with_resource(resource).with_resource_id(id.to_string());
            audit::deletion!(ctx);
            Ok(())
        }
        Err(e) => {
            tracing::error!("Error deleting API key: {:?}", e);
            Err(e)
        }
    }
}

// ── Admin Keys ──────────────────────────────────────────────────────────────

pub async fn create_admin_key(
    key_hash: &str,
    label: Option<String>,
    permissions: Vec<String>,
    ctx: AuditContext,
) -> Result<AdminKey> {
    let svc = service();
    match svc.create_admin_key(key_hash, label, permissions).await {
        Ok(admin_key) => {
            let resource = ADMIN_KEY.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(admin_key.id.to_string());
            audit::creation!(ctx);
            Ok(admin_key)
        }
        Err(e) => {
            tracing::error!("Error creating admin key: {:?}", e);
            Err(e)
        }
    }
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
    let svc = service();
    match svc.update_admin_key(admin_key).await {
        Ok(updated) => {
            let resource = ADMIN_KEY.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(updated.id.clone());
            audit::modification!(ctx);
            Ok(updated)
        }
        Err(e) => {
            tracing::error!("Error updating admin key: {:?}", e);
            Err(e)
        }
    }
}

pub async fn revoke_admin_key(id: &str, ctx: AuditContext) -> Result<()> {
    let svc = service();
    match svc.revoke_admin_key(id).await {
        Ok(()) => {
            let resource = ADMIN_KEY.to_string();
            let ctx = ctx.with_resource(resource).with_resource_id(id.to_string());
            audit::modification!(ctx);
            Ok(())
        }
        Err(e) => {
            tracing::error!("Error revoking admin key: {:?}", e);
            Err(e)
        }
    }
}

pub async fn delete_admin_key(id: &str, ctx: AuditContext) -> Result<()> {
    let svc = service();
    match svc.delete_admin_key(id).await {
        Ok(()) => {
            let resource = ADMIN_KEY.to_string();
            let ctx = ctx.with_resource(resource).with_resource_id(id.to_string());
            audit::deletion!(ctx);
            Ok(())
        }
        Err(e) => {
            tracing::error!("Error deleting admin key: {:?}", e);
            Err(e)
        }
    }
}

// ── Service Accounts ────────────────────────────────────────────────────────

pub async fn create_service_account(
    name: &str,
    description: Option<&str>,
    org_id: Option<&str>,
    ctx: AuditContext,
) -> Result<ServiceAccount> {
    let svc = service();
    match svc.create_service_account(name, description, org_id).await {
        Ok(sa) => {
            let metadata = serde_json::to_value(&sa).unwrap_or_default();
            let resource = SERVICE_ACCOUNT.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(sa.id.to_string())
                .with_metadata(metadata);
            audit::creation!(ctx);
            Ok(sa)
        }
        Err(e) => {
            tracing::error!("Error creating service account: {:?}", e);
            Err(e)
        }
    }
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
    let svc = service();
    match svc
        .update_service_account(id, name, description, None)
        .await
    {
        Ok(sa) => {
            let metadata = serde_json::to_value(&sa).unwrap_or_default();
            let resource = SERVICE_ACCOUNT.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(sa.id.clone())
                .with_metadata(metadata);
            audit::modification!(ctx);
            Ok(sa)
        }
        Err(e) => {
            tracing::error!("Error updating service account: {:?}", e);
            Err(e)
        }
    }
}

pub async fn delete_service_account(id: &str, ctx: AuditContext) -> Result<()> {
    let svc = service();
    match svc.delete_service_account(id).await {
        Ok(()) => {
            let resource = SERVICE_ACCOUNT.to_string();
            let ctx = ctx.with_resource(resource).with_resource_id(id.to_string());
            audit::deletion!(ctx);
            Ok(())
        }
        Err(e) => {
            tracing::error!("Error deleting service account: {:?}", e);
            Err(e)
        }
    }
}

// ── Organizations ───────────────────────────────────────────────────────────

pub async fn create_organization(
    name: &str,
    description: Option<&str>,
    attrs: Option<&serde_json::Value>,
    ctx: AuditContext,
) -> Result<Organization> {
    let svc = service();
    match svc.create_organization(name, description, attrs).await {
        Ok(org) => {
            let metadata = serde_json::to_value(&org).unwrap_or_default();
            let resource = ORGANIZATION.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(org.id.to_string())
                .with_metadata(metadata);
            audit::creation!(ctx);
            Ok(org)
        }
        Err(e) => {
            tracing::error!("Error creating organization: {:?}", e);
            Err(e)
        }
    }
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
    let svc = service();
    match svc.update_organization(id, name, description, attrs).await {
        Ok(org) => {
            let metadata = serde_json::to_value(&org).unwrap_or_default();
            let resource = ORGANIZATION.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(org.id.clone())
                .with_metadata(metadata);
            audit::modification!(ctx);
            Ok(org)
        }
        Err(e) => {
            tracing::error!("Error updating organization: {:?}", e);
            Err(e)
        }
    }
}

pub async fn delete_organization(id: &str, ctx: AuditContext) -> Result<()> {
    let svc = service();
    match svc.delete_organization(id).await {
        Ok(()) => {
            let resource = ORGANIZATION.to_string();
            let ctx = ctx.with_resource(resource).with_resource_id(id.to_string());
            audit::deletion!(ctx);
            Ok(())
        }
        Err(e) => {
            tracing::error!("Error deleting organization: {:?}", e);
            Err(e)
        }
    }
}

pub async fn add_user_to_organization(
    user_id: &str,
    org_id: &str,
    ctx: AuditContext,
) -> Result<()> {
    let svc = service();
    match svc.add_user_to_organization(user_id, org_id).await {
        Ok(()) => {
            let resource = ORGANIZATION.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(org_id.to_string());
            audit::modification!(ctx);
            Ok(())
        }
        Err(e) => {
            tracing::error!("Error adding user to organization: {:?}", e);
            Err(e)
        }
    }
}

pub async fn remove_user_from_organization(
    user_id: &str,
    org_id: &str,
    ctx: AuditContext,
) -> Result<()> {
    let svc = service();
    match svc.remove_user_from_organization(user_id, org_id).await {
        Ok(()) => {
            let resource = ORGANIZATION.to_string();
            let ctx = ctx
                .with_resource(resource)
                .with_resource_id(org_id.to_string());
            audit::modification!(ctx);
            Ok(())
        }
        Err(e) => {
            tracing::error!("Error removing user from organization: {:?}", e);
            Err(e)
        }
    }
}

pub async fn get_organization_users(org_id: &str) -> Result<Vec<User>> {
    let svc = service();
    svc.get_organization_users(org_id).await
}

pub async fn get_organization_users_paginated(
    org_id: &str,
    limit: i64,
    offset: i64,
) -> Result<Vec<User>> {
    let svc = service();
    svc.get_organization_users_paginated(org_id, limit, offset)
        .await
}

pub async fn count_organization_users(org_id: &str) -> Result<i64> {
    let svc = service();
    svc.count_organization_users(org_id).await
}

pub async fn get_user_organizations(user_id: &str) -> Result<Vec<Organization>> {
    let svc = service();
    svc.get_user_organizations(user_id).await
}
