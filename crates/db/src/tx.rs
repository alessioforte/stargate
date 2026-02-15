use crate::ent::{
    AdminKey, ApiKey, Credential, CredentialType, Organization, Profile, ServiceAccount, User,
};
use anyhow::Result;
use serde_json::Value as JsonValue;

#[async_trait::async_trait]
pub trait Transaction {
    // ── Users ───────────────────────────────────────────────────────────────
    async fn create_user(
        &self,
        user: Profile,
        credential_type: CredentialType,
        value: &str,
    ) -> Result<User>;
    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>>;
    async fn get_user_by_id(&self, id: &str) -> Result<Option<User>>;
    async fn get_all_users(&self, limit: i64, offset: i64) -> Result<Vec<User>>;
    async fn count_users(&self) -> Result<i64>;
    async fn search_users(&self, query: &str, limit: i64, offset: i64) -> Result<Vec<User>>;
    async fn count_search_users(&self, query: &str) -> Result<i64>;
    async fn update_user(&self, user: User) -> Result<User>;
    async fn delete_user(&self, id: &str) -> Result<()>;
    async fn change_password(&self, user_id: &str, new_password: &str) -> Result<Credential>;
    async fn get_credential(
        &self,
        user_id: &str,
        credential_type: CredentialType,
    ) -> Result<Option<Credential>>;

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
    ) -> Result<ApiKey>;
    async fn create_service_account_api_key(
        &self,
        service_account_id: &str,
        key_hash: &str,
        label: &str,
        attrs: Option<JsonValue>,
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
    async fn update_api_key(&self, api_key: ApiKey) -> Result<ApiKey>;
    async fn revoke_api_key(&self, id: &str) -> Result<()>;
    async fn delete_api_key(&self, id: &str) -> Result<()>;

    // ── Admin Keys ──────────────────────────────────────────────────────────
    async fn create_admin_key(
        &self,
        key_hash: &str,
        label: Option<String>,
        permissions: Vec<String>,
    ) -> Result<AdminKey>;
    async fn get_admin_key_by_hash(&self, key_hash: &str) -> Result<Option<AdminKey>>;
    async fn get_admin_key_by_id(&self, id: &str) -> Result<Option<AdminKey>>;
    async fn get_all_admin_keys(&self, limit: i64, offset: i64) -> Result<Vec<AdminKey>>;
    async fn count_admin_keys(&self) -> Result<i64>;
    async fn update_admin_key(&self, admin_key: AdminKey) -> Result<AdminKey>;
    async fn revoke_admin_key(&self, id: &str) -> Result<()>;
    async fn delete_admin_key(&self, id: &str) -> Result<()>;

    // ── Service Accounts ────────────────────────────────────────────────────
    async fn create_service_account(
        &self,
        name: &str,
        description: Option<&str>,
        org_id: Option<&str>,
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
    ) -> Result<ServiceAccount>;
    async fn delete_service_account(&self, id: &str) -> Result<()>;

    // ── Organizations ───────────────────────────────────────────────────────
    async fn create_organization(
        &self,
        name: &str,
        description: Option<&str>,
        attrs: Option<&serde_json::Value>,
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
    ) -> Result<Organization>;
    async fn delete_organization(&self, id: &str) -> Result<()>;
    async fn add_user_to_organization(&self, user_id: &str, org_id: &str) -> Result<()>;
    async fn remove_user_from_organization(&self, user_id: &str, org_id: &str) -> Result<()>;
    async fn get_organization_users(&self, org_id: &str) -> Result<Vec<User>>;
    async fn get_organization_users_paginated(
        &self,
        org_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<User>>;
    async fn count_organization_users(&self, org_id: &str) -> Result<i64>;
    async fn get_user_organizations(&self, user_id: &str) -> Result<Vec<Organization>>;

    // ── Audit ───────────────────────────────────────────────────────────────
    async fn insert_audit_log_bulk(&self, logs: Vec<crate::ent::Audit>) -> Result<()>;
}
