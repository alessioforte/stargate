use crate::ent::{
    AdminKey, AdminOverviewStats, ApiKey, ApiKeyAuth, Credential, CredentialHistory,
    CredentialType, OAuthClient, OAuthConsent, OrgMember, OrgMembership, Organization, Profile,
    ServiceAccount, SuperAdmin, TrustedAuditContext, User,
};
use anyhow::Result;
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct InstanceBootstrapResult {
    pub super_admin_created: bool,
    pub oauth_client_created: bool,
}

#[async_trait::async_trait]
pub trait DbStore {
    // ── Admin overview ──────────────────────────────────────────────────────
    async fn get_admin_overview_stats(&self) -> Result<AdminOverviewStats>;

    // ── Users ───────────────────────────────────────────────────────────────
    async fn bootstrap_instance(
        &self,
        profile: Option<Profile>,
        password_hash: Option<&str>,
        admin_client: OAuthClient,
        ctx: TrustedAuditContext,
    ) -> Result<InstanceBootstrapResult>;
    async fn create_user(
        &self,
        user: Profile,
        credential_type: CredentialType,
        value: &str,
        ctx: TrustedAuditContext,
    ) -> Result<User>;
    async fn create_super_admin_user(
        &self,
        user: Profile,
        credential_type: CredentialType,
        value: &str,
        ctx: TrustedAuditContext,
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
    async fn update_user(&self, user: User, ctx: TrustedAuditContext) -> Result<User>;
    /// Deletes the user and revokes their API keys in the same transaction
    /// (the FK cascade only removes ownership links, not the key rows).
    /// Returns the revoked key hashes so callers can purge cached subjects.
    async fn delete_user(&self, id: &str, ctx: TrustedAuditContext) -> Result<Vec<String>>;
    async fn change_password(
        &self,
        user_id: &str,
        new_password: &str,
        ctx: TrustedAuditContext,
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
        ctx: TrustedAuditContext,
    ) -> Result<bool>;

    // ── OAuth Clients ──────────────────────────────────────────────────────
    async fn create_oauth_client(
        &self,
        client: OAuthClient,
        ctx: TrustedAuditContext,
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
        ctx: TrustedAuditContext,
    ) -> Result<OAuthClient>;
    async fn update_oauth_client_secret_hash(
        &self,
        client_id: &str,
        client_secret_hash: Option<&str>,
        ctx: TrustedAuditContext,
    ) -> Result<OAuthClient>;
    async fn set_oauth_client_enabled(
        &self,
        client_id: &str,
        enabled: bool,
        ctx: TrustedAuditContext,
    ) -> Result<OAuthClient>;
    async fn delete_oauth_client(&self, client_id: &str, ctx: TrustedAuditContext) -> Result<()>;

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
        org_id: Option<&str>,
        ctx: TrustedAuditContext,
    ) -> Result<ApiKey>;
    async fn create_service_account_api_key(
        &self,
        service_account_id: &str,
        key_hash: &str,
        label: &str,
        attrs: Option<JsonValue>,
        ctx: TrustedAuditContext,
    ) -> Result<ApiKey>;
    async fn get_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>>;
    /// API key joined with its owner's org binding, for gateway auth.
    async fn get_api_key_auth_by_hash(&self, key_hash: &str) -> Result<Option<ApiKeyAuth>>;
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
    async fn search_api_keys(&self, query: &str, limit: i64, offset: i64) -> Result<Vec<ApiKey>>;
    async fn count_search_api_keys(&self, query: &str) -> Result<i64>;
    async fn update_api_key(&self, api_key: ApiKey, ctx: TrustedAuditContext) -> Result<ApiKey>;
    async fn revoke_api_key(&self, id: &str, ctx: TrustedAuditContext) -> Result<()>;
    async fn delete_api_key(&self, id: &str, ctx: TrustedAuditContext) -> Result<()>;

    // ── Admin Keys ──────────────────────────────────────────────────────────
    async fn create_admin_key(
        &self,
        key_hash: &str,
        label: Option<String>,
        permissions: Vec<String>,
        ctx: TrustedAuditContext,
    ) -> Result<AdminKey>;
    async fn get_admin_key_by_hash(&self, key_hash: &str) -> Result<Option<AdminKey>>;
    async fn get_admin_key_by_id(&self, id: &str) -> Result<Option<AdminKey>>;
    async fn get_all_admin_keys(&self, limit: i64, offset: i64) -> Result<Vec<AdminKey>>;
    async fn count_admin_keys(&self) -> Result<i64>;
    async fn update_admin_key(
        &self,
        admin_key: AdminKey,
        ctx: TrustedAuditContext,
    ) -> Result<AdminKey>;
    async fn revoke_admin_key(&self, id: &str, ctx: TrustedAuditContext) -> Result<()>;
    async fn delete_admin_key(&self, id: &str, ctx: TrustedAuditContext) -> Result<()>;

    // ── Service Accounts ────────────────────────────────────────────────────
    async fn create_service_account(
        &self,
        name: &str,
        description: Option<&str>,
        org_id: Option<&str>,
        ctx: TrustedAuditContext,
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
        ctx: TrustedAuditContext,
    ) -> Result<ServiceAccount>;
    /// Deletes the service account and revokes its API keys in the same
    /// transaction. Returns the revoked key hashes.
    async fn delete_service_account(
        &self,
        id: &str,
        ctx: TrustedAuditContext,
    ) -> Result<Vec<String>>;

    // ── Organizations ───────────────────────────────────────────────────────
    async fn create_organization(
        &self,
        name: &str,
        description: Option<&str>,
        attrs: Option<&serde_json::Value>,
        ctx: TrustedAuditContext,
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
        ctx: TrustedAuditContext,
    ) -> Result<Organization>;
    /// Deletes the organization and revokes every key acting in it (user
    /// keys bound to the org, keys of its cascading service accounts).
    /// Returns the revoked key hashes.
    async fn delete_organization(&self, id: &str, ctx: TrustedAuditContext) -> Result<Vec<String>>;
    /// Upsert: adds the membership or updates the role of an existing one.
    async fn add_user_to_organization(
        &self,
        user_id: &str,
        org_id: &str,
        role: &str,
        ctx: TrustedAuditContext,
    ) -> Result<()>;
    /// Removes the membership and revokes the user's API keys bound to that
    /// org in the same transaction. Returns the revoked key hashes.
    async fn remove_user_from_organization(
        &self,
        user_id: &str,
        org_id: &str,
        ctx: TrustedAuditContext,
    ) -> Result<Vec<String>>;
    async fn get_organization_users(&self, org_id: &str) -> Result<Vec<OrgMember>>;
    async fn get_organization_users_paginated(
        &self,
        org_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<OrgMember>>;
    async fn count_organization_users(&self, org_id: &str) -> Result<i64>;
    async fn get_user_organizations(&self, user_id: &str) -> Result<Vec<OrgMembership>>;
    async fn get_user_organization(
        &self,
        user_id: &str,
        org_id: &str,
    ) -> Result<Option<OrgMembership>>;
}
