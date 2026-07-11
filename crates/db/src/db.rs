use crate::ent::{
    AdminKey, ApiKey, AuditContext, Credential, CredentialHistory, CredentialType, OAuthClient,
    OAuthConsent, Organization, Profile, ServiceAccount, SuperAdmin, User,
};
use anyhow::Result;
use serde_json::Value as JsonValue;

#[async_trait::async_trait]
pub trait DbStore {
    // ── Users ───────────────────────────────────────────────────────────────
    async fn create_user(
        &self,
        user: Profile,
        credential_type: CredentialType,
        value: &str,
        ctx: AuditContext,
    ) -> Result<User>;
    async fn create_super_admin_user(
        &self,
        user: Profile,
        credential_type: CredentialType,
        value: &str,
        ctx: AuditContext,
    ) -> Result<User>;
    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>>;
    async fn get_user_by_id(&self, id: &str) -> Result<Option<User>>;
    async fn get_all_users(&self, limit: i64, offset: i64) -> Result<Vec<User>>;
    async fn count_users(&self) -> Result<i64>;
    async fn get_super_admin_users(&self) -> Result<Vec<User>>;
    async fn get_super_admin_by_user_id(&self, user_id: &str) -> Result<Option<SuperAdmin>>;
    async fn super_admin_exists(&self) -> Result<bool>;
    async fn search_users(&self, query: &str, limit: i64, offset: i64) -> Result<Vec<User>>;
    async fn count_search_users(&self, query: &str) -> Result<i64>;
    async fn update_user(&self, user: User, ctx: AuditContext) -> Result<User>;
    async fn delete_user(&self, id: &str, ctx: AuditContext) -> Result<()>;
    async fn change_password(
        &self,
        user_id: &str,
        new_password: &str,
        ctx: AuditContext,
    ) -> Result<Credential>;
    async fn get_credential(
        &self,
        user_id: &str,
        credential_type: CredentialType,
    ) -> Result<Option<Credential>>;
    /// Most recent previous password hashes, newest first.
    async fn get_credential_history(
        &self,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<CredentialHistory>>;
    /// Upgrade a stored password hash in place (same password, new hash
    /// parameters). Does not touch `updated_at` or history. Returns `false`
    /// when the stored hash no longer matches `old_value`.
    async fn rehash_credential(
        &self,
        user_id: &str,
        old_value: &str,
        new_value: &str,
        ctx: AuditContext,
    ) -> Result<bool>;

    // ── OAuth Clients ──────────────────────────────────────────────────────
    async fn create_oauth_client(
        &self,
        client: OAuthClient,
        ctx: AuditContext,
    ) -> Result<OAuthClient>;
    async fn get_oauth_client_by_client_id(&self, client_id: &str) -> Result<Option<OAuthClient>>;
    async fn get_all_oauth_clients(&self, limit: i64, offset: i64) -> Result<Vec<OAuthClient>>;
    async fn count_oauth_clients(&self) -> Result<i64>;
    async fn search_oauth_clients(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<OAuthClient>>;
    async fn count_search_oauth_clients(&self, query: &str) -> Result<i64>;
    async fn update_oauth_client(
        &self,
        client: OAuthClient,
        ctx: AuditContext,
    ) -> Result<OAuthClient>;
    async fn update_oauth_client_secret_hash(
        &self,
        client_id: &str,
        client_secret_hash: Option<&str>,
        ctx: AuditContext,
    ) -> Result<OAuthClient>;
    async fn set_oauth_client_enabled(
        &self,
        client_id: &str,
        enabled: bool,
        ctx: AuditContext,
    ) -> Result<OAuthClient>;
    async fn delete_oauth_client(&self, client_id: &str, ctx: AuditContext) -> Result<()>;

    // ── OAuth Consents ─────────────────────────────────────────────────────
    async fn upsert_oauth_consent(&self, consent: OAuthConsent) -> Result<OAuthConsent>;
    async fn get_active_oauth_consent(
        &self,
        user_id: &str,
        client_id: &str,
    ) -> Result<Option<OAuthConsent>>;
    async fn revoke_oauth_consent(&self, user_id: &str, client_id: &str) -> Result<()>;

    // ── API Keys ────────────────────────────────────────────────────────────
    async fn create_api_key(
        &self,
        key_hash: &str,
        label: &str,
        attrs: Option<JsonValue>,
    ) -> Result<ApiKey>;
    async fn create_user_api_key(
        &self,
        user_id: &str,
        key_hash: &str,
        label: &str,
        attrs: Option<JsonValue>,
        ctx: AuditContext,
    ) -> Result<ApiKey>;
    async fn create_service_account_api_key(
        &self,
        service_account_id: &str,
        key_hash: &str,
        label: &str,
        attrs: Option<JsonValue>,
        ctx: AuditContext,
    ) -> Result<ApiKey>;
    async fn get_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>>;
    async fn get_api_key_by_id(&self, id: &str) -> Result<Option<ApiKey>>;
    async fn get_all_api_keys(&self, limit: i64, offset: i64) -> Result<Vec<ApiKey>>;
    async fn get_api_keys_by_user_id(&self, user_id: &str) -> Result<Vec<ApiKey>>;
    async fn get_all_user_api_keys(&self, limit: i64, offset: i64) -> Result<Vec<ApiKey>>;
    async fn count_user_api_keys(&self) -> Result<i64>;
    async fn get_api_keys_by_service_account_id(
        &self,
        service_account_id: &str,
    ) -> Result<Vec<ApiKey>>;
    async fn get_all_service_account_api_keys(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ApiKey>>;
    async fn count_service_account_api_keys(&self) -> Result<i64>;
    async fn count_api_keys(&self) -> Result<i64>;
    async fn update_api_key(&self, api_key: ApiKey, ctx: AuditContext) -> Result<ApiKey>;
    async fn revoke_api_key(&self, id: &str, ctx: AuditContext) -> Result<()>;
    async fn delete_api_key(&self, id: &str, ctx: AuditContext) -> Result<()>;

    // ── Admin Keys ──────────────────────────────────────────────────────────
    async fn create_admin_key(
        &self,
        key_hash: &str,
        label: Option<String>,
        permissions: Vec<String>,
        ctx: AuditContext,
    ) -> Result<AdminKey>;
    async fn get_admin_key_by_hash(&self, key_hash: &str) -> Result<Option<AdminKey>>;
    async fn get_admin_key_by_id(&self, id: &str) -> Result<Option<AdminKey>>;
    async fn get_all_admin_keys(&self, limit: i64, offset: i64) -> Result<Vec<AdminKey>>;
    async fn count_admin_keys(&self) -> Result<i64>;
    async fn update_admin_key(&self, admin_key: AdminKey, ctx: AuditContext) -> Result<AdminKey>;
    async fn revoke_admin_key(&self, id: &str, ctx: AuditContext) -> Result<()>;
    async fn delete_admin_key(&self, id: &str, ctx: AuditContext) -> Result<()>;

    // ── Service Accounts ────────────────────────────────────────────────────
    async fn create_service_account(
        &self,
        name: &str,
        description: Option<&str>,
        org_id: Option<&str>,
        ctx: AuditContext,
    ) -> Result<ServiceAccount>;
    async fn get_service_account_by_id(&self, id: &str) -> Result<Option<ServiceAccount>>;
    async fn get_all_service_accounts(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ServiceAccount>>;
    async fn count_service_accounts(&self) -> Result<i64>;
    async fn search_service_accounts(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ServiceAccount>>;
    async fn count_search_service_accounts(&self, query: &str) -> Result<i64>;
    async fn update_service_account(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        org_id: Option<&str>,
        ctx: AuditContext,
    ) -> Result<ServiceAccount>;
    async fn delete_service_account(&self, id: &str, ctx: AuditContext) -> Result<()>;

    // ── Organizations ───────────────────────────────────────────────────────
    async fn create_organization(
        &self,
        name: &str,
        description: Option<&str>,
        attrs: Option<&serde_json::Value>,
        ctx: AuditContext,
    ) -> Result<Organization>;
    async fn get_organization_by_id(&self, id: &str) -> Result<Option<Organization>>;
    async fn get_all_organizations(&self, limit: i64, offset: i64) -> Result<Vec<Organization>>;
    async fn count_organizations(&self) -> Result<i64>;
    async fn search_organizations(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Organization>>;
    async fn count_search_organizations(&self, query: &str) -> Result<i64>;
    async fn update_organization(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        attrs: Option<&serde_json::Value>,
        ctx: AuditContext,
    ) -> Result<Organization>;
    async fn delete_organization(&self, id: &str, ctx: AuditContext) -> Result<()>;
    async fn add_user_to_organization(
        &self,
        user_id: &str,
        org_id: &str,
        ctx: AuditContext,
    ) -> Result<()>;
    async fn remove_user_from_organization(
        &self,
        user_id: &str,
        org_id: &str,
        ctx: AuditContext,
    ) -> Result<()>;
    async fn get_organization_users(&self, org_id: &str) -> Result<Vec<User>>;
    async fn get_organization_users_paginated(
        &self,
        org_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<User>>;
    async fn count_organization_users(&self, org_id: &str) -> Result<i64>;
    async fn get_user_organizations(&self, user_id: &str) -> Result<Vec<Organization>>;
}
