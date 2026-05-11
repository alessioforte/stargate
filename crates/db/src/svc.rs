use super::repo::{
    AdminKeyRepository, ApiKeyRepository, AuditRepository, CredentialRepository,
    OAuthClientRepository, OAuthConsentRepository, OrganizationRepository,
    ServiceAccountRepository, SuperAdminRepository, UserRepository,
};
use crate::backend::Pool;
use crate::ent::{
    AdminKey, ApiKey, Credential, CredentialType, OAuthClient, OAuthConsent, Organization, Profile,
    ServiceAccount, SuperAdmin, User,
};
use crate::tx::Transaction;
use anyhow::Result;
use serde_json::Value as JsonValue;
use std::fs;

#[derive(Clone)]
pub struct Service {
    pool: Pool,
    admin_key: AdminKeyRepository,
    user: UserRepository,
    credential: CredentialRepository,
    oauth_client: OAuthClientRepository,
    oauth_consent: OAuthConsentRepository,
    api_key: ApiKeyRepository,
    audit: AuditRepository,
    service_account: ServiceAccountRepository,
    organization: OrganizationRepository,
    super_admin: SuperAdminRepository,
}

impl Service {
    pub fn new(pool: Pool) -> Self {
        Self {
            pool,
            admin_key: AdminKeyRepository::new(),
            user: UserRepository::new(),
            credential: CredentialRepository::new(),
            oauth_client: OAuthClientRepository::new(),
            oauth_consent: OAuthConsentRepository::new(),
            api_key: ApiKeyRepository::new(),
            audit: AuditRepository::new(),
            service_account: ServiceAccountRepository::new(),
            organization: OrganizationRepository::new(),
            super_admin: SuperAdminRepository::new(),
        }
    }

    pub async fn init_schema(&self, file: &str) {
        let ddl = match fs::read_to_string(file) {
            Ok(content) => content,
            Err(e) => {
                eprintln!("Failed to read schema file '{}': {}", file, e);
                return;
            }
        };

        // Execute the entire DDL as a single batch. This correctly handles
        // PL/pgSQL DO $$ ... END $$; blocks that contain semicolons.
        if let Err(e) = sqlx::raw_sql(&ddl).execute(&self.pool).await {
            eprintln!("Failed to execute schema from '{}': {}", file, e);
        }
    }

    /// Lightweight connectivity check — executes `SELECT 1`.
    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }
}

/// Connect to the `postgres` maintenance database and create the target
/// database if it does not already exist.
#[cfg(feature = "postgres")]
pub async fn ensure_database(user: &str, password: &str, host: &str, database: &str) {
    let maintenance_url = format!("postgres://{}:{}@{}/postgres", user, password, host);
    let pool = match sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&maintenance_url)
        .await
    {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Cannot connect to PostgreSQL server: {}", e);
            return;
        }
    };

    // Check if the database exists (parameterized query to avoid injection)
    let exists: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_database WHERE datname = $1)")
            .bind(database)
            .fetch_one(&pool)
            .await
            .unwrap_or(false);

    if !exists {
        // CREATE DATABASE cannot use bind parameters, but `database` comes
        // from an env var set by the operator, not from user input.
        let stmt = format!("CREATE DATABASE \"{}\"", database);
        match sqlx::query(&stmt).execute(&pool).await {
            Ok(_) => eprintln!("Created database '{}'", database),
            Err(e) => eprintln!("Failed to create database '{}': {}", database, e),
        }
    }

    pool.close().await;
}

// Generic init function that works with any database URL
pub async fn init(conn: &str) -> Result<Service> {
    let max_connections: u32 = std::env::var("DB_POOL_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20);

    #[cfg(feature = "sqlite")]
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(max_connections)
        .connect(conn)
        .await?;

    #[cfg(feature = "postgres")]
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(max_connections)
        .connect(conn)
        .await?;

    let service = Service::new(pool);

    #[cfg(feature = "postgres")]
    service.init_schema("ddl/postgres.sql").await;

    #[cfg(feature = "sqlite")]
    service.init_schema("ddl/sqlite.sql").await;

    Ok(service)
}

#[async_trait::async_trait]
impl Transaction for Service {
    // ── Users ───────────────────────────────────────────────────────────────

    async fn create_user(
        &self,
        profile: Profile,
        credential_type: CredentialType,
        value: &str,
    ) -> Result<User> {
        let mut tx = self.pool.begin().await?;

        let user = User::new(profile.email, profile.nickname)
            .given_name(profile.given_name)
            .family_name(profile.family_name)
            .picture(profile.picture)
            .phone_number(profile.phone_number)
            .attrs(profile.attrs);

        let user_id = user.id.clone();
        let record = self.user.create(&mut tx, user).await?;
        self.credential
            .create(&mut tx, &user_id, credential_type, value)
            .await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn create_super_admin_user(
        &self,
        profile: Profile,
        credential_type: CredentialType,
        value: &str,
    ) -> Result<User> {
        let mut tx = self.pool.begin().await?;

        let user = User::new(profile.email, profile.nickname)
            .given_name(profile.given_name)
            .family_name(profile.family_name)
            .picture(profile.picture)
            .phone_number(profile.phone_number)
            .attrs(profile.attrs);

        let user_id = user.id.clone();
        let record = self.user.create(&mut tx, user).await?;
        self.credential
            .create(&mut tx, &user_id, credential_type, value)
            .await?;
        self.super_admin.create(&mut tx, &user_id).await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        self.user.get_by_username(&self.pool, username).await
    }

    async fn get_user_by_id(&self, id: &str) -> Result<Option<User>> {
        self.user.get_by_id(&self.pool, id).await
    }

    async fn get_all_users(&self, limit: i64, offset: i64) -> Result<Vec<User>> {
        self.user.get_all(&self.pool, limit, offset).await
    }

    async fn count_users(&self) -> Result<i64> {
        self.user.count(&self.pool).await
    }

    async fn get_super_admin_by_user_id(&self, user_id: &str) -> Result<Option<SuperAdmin>> {
        self.super_admin.get_by_user_id(&self.pool, user_id).await
    }

    async fn super_admin_exists(&self) -> Result<bool> {
        Ok(self.super_admin.count_active(&self.pool).await? > 0)
    }

    async fn search_users(&self, query: &str, limit: i64, offset: i64) -> Result<Vec<User>> {
        self.user
            .search_with_pagination(&self.pool, query, limit, offset)
            .await
    }

    async fn count_search_users(&self, query: &str) -> Result<i64> {
        self.user.count_search(&self.pool, query).await
    }

    async fn update_user(&self, user: User) -> Result<User> {
        let mut tx = self.pool.begin().await?;
        let updated_user = self.user.update(&mut tx, user).await?;
        tx.commit().await?;
        Ok(updated_user)
    }

    async fn delete_user(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.user.delete(&mut tx, id).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn change_password(&self, user_id: &str, new_password: &str) -> Result<Credential> {
        let mut tx = self.pool.begin().await?;
        let credential = self
            .credential
            .change_password(&mut tx, user_id, new_password)
            .await?;
        tx.commit().await?;
        Ok(credential)
    }

    async fn get_credential(
        &self,
        user_id: &str,
        credential_type: CredentialType,
    ) -> Result<Option<Credential>> {
        self.credential
            .get_by_user_id(&self.pool, user_id, credential_type)
            .await
    }

    // ── OAuth Clients ──────────────────────────────────────────────────────

    async fn create_oauth_client(&self, client: OAuthClient) -> Result<OAuthClient> {
        let mut tx = self.pool.begin().await?;
        let client = self.oauth_client.create(&mut tx, client).await?;
        tx.commit().await?;
        Ok(client)
    }

    async fn get_oauth_client_by_client_id(&self, client_id: &str) -> Result<Option<OAuthClient>> {
        self.oauth_client
            .get_by_client_id(&self.pool, client_id)
            .await
    }

    async fn get_all_oauth_clients(&self, limit: i64, offset: i64) -> Result<Vec<OAuthClient>> {
        self.oauth_client.get_all(&self.pool, limit, offset).await
    }

    async fn count_oauth_clients(&self) -> Result<i64> {
        self.oauth_client.count(&self.pool).await
    }

    async fn search_oauth_clients(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<OAuthClient>> {
        self.oauth_client
            .search(&self.pool, query, limit, offset)
            .await
    }

    async fn count_search_oauth_clients(&self, query: &str) -> Result<i64> {
        self.oauth_client.count_search(&self.pool, query).await
    }

    async fn update_oauth_client(&self, client: OAuthClient) -> Result<OAuthClient> {
        let mut tx = self.pool.begin().await?;
        let client = self.oauth_client.update(&mut tx, client).await?;
        tx.commit().await?;
        Ok(client)
    }

    async fn update_oauth_client_secret_hash(
        &self,
        client_id: &str,
        client_secret_hash: Option<&str>,
    ) -> Result<OAuthClient> {
        let mut tx = self.pool.begin().await?;
        let client = self
            .oauth_client
            .update_secret_hash(&mut tx, client_id, client_secret_hash)
            .await?;
        tx.commit().await?;
        Ok(client)
    }

    async fn set_oauth_client_enabled(
        &self,
        client_id: &str,
        enabled: bool,
    ) -> Result<OAuthClient> {
        let mut tx = self.pool.begin().await?;
        let client = self
            .oauth_client
            .set_enabled(&mut tx, client_id, enabled)
            .await?;
        tx.commit().await?;
        Ok(client)
    }

    async fn delete_oauth_client(&self, client_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.oauth_client
            .delete_by_client_id(&mut tx, client_id)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    // ── OAuth Consents ─────────────────────────────────────────────────────

    async fn upsert_oauth_consent(&self, consent: OAuthConsent) -> Result<OAuthConsent> {
        let mut tx = self.pool.begin().await?;
        let consent = self.oauth_consent.upsert(&mut tx, consent).await?;
        tx.commit().await?;
        Ok(consent)
    }

    async fn get_active_oauth_consent(
        &self,
        user_id: &str,
        client_id: &str,
    ) -> Result<Option<OAuthConsent>> {
        self.oauth_consent
            .get_active_for_user_client(&self.pool, user_id, client_id, chrono::Utc::now())
            .await
    }

    async fn revoke_oauth_consent(&self, user_id: &str, client_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.oauth_consent
            .revoke_for_user_client(&mut tx, user_id, client_id)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    // ── API Keys ────────────────────────────────────────────────────────────

    async fn create_api_key(
        &self,
        key_hash: &str,
        label: &str,
        attrs: Option<JsonValue>,
    ) -> Result<ApiKey> {
        let mut tx = self.pool.begin().await?;
        let api_key = self.api_key.create(&mut tx, key_hash, label, attrs).await?;
        tx.commit().await?;
        Ok(api_key)
    }

    async fn create_user_api_key(
        &self,
        user_id: &str,
        key_hash: &str,
        label: &str,
        attrs: Option<JsonValue>,
    ) -> Result<ApiKey> {
        let mut tx = self.pool.begin().await?;
        let api_key = self.api_key.create(&mut tx, key_hash, label, attrs).await?;
        self.api_key
            .link_to_user(&mut tx, &api_key.id, user_id)
            .await?;
        tx.commit().await?;
        Ok(api_key)
    }

    async fn create_service_account_api_key(
        &self,
        service_account_id: &str,
        key_hash: &str,
        label: &str,
        attrs: Option<JsonValue>,
    ) -> Result<ApiKey> {
        let mut tx = self.pool.begin().await?;
        let api_key = self.api_key.create(&mut tx, key_hash, label, attrs).await?;
        self.api_key
            .link_to_service_account(&mut tx, &api_key.id, service_account_id)
            .await?;
        tx.commit().await?;
        Ok(api_key)
    }

    async fn get_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        self.api_key.get_by_hash(&self.pool, key_hash).await
    }

    async fn get_api_key_by_id(&self, id: &str) -> Result<Option<ApiKey>> {
        self.api_key.get_by_id(&self.pool, id).await
    }

    async fn get_all_api_keys(&self, limit: i64, offset: i64) -> Result<Vec<ApiKey>> {
        self.api_key.get_all(&self.pool, limit, offset).await
    }

    async fn get_api_keys_by_user_id(&self, user_id: &str) -> Result<Vec<ApiKey>> {
        self.api_key.get_by_user_id(&self.pool, user_id).await
    }

    async fn get_all_user_api_keys(&self, limit: i64, offset: i64) -> Result<Vec<ApiKey>> {
        self.api_key
            .get_all_by_user_type(&self.pool, limit, offset)
            .await
    }

    async fn count_user_api_keys(&self) -> Result<i64> {
        self.api_key.count_by_user_type(&self.pool).await
    }

    async fn get_api_keys_by_service_account_id(
        &self,
        service_account_id: &str,
    ) -> Result<Vec<ApiKey>> {
        self.api_key
            .get_by_service_account_id(&self.pool, service_account_id)
            .await
    }

    async fn get_all_service_account_api_keys(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ApiKey>> {
        self.api_key
            .get_all_by_service_account_type(&self.pool, limit, offset)
            .await
    }

    async fn count_service_account_api_keys(&self) -> Result<i64> {
        self.api_key.count_by_service_account_type(&self.pool).await
    }

    async fn count_api_keys(&self) -> Result<i64> {
        self.api_key.count(&self.pool).await
    }

    async fn update_api_key(&self, api_key: ApiKey) -> Result<ApiKey> {
        let mut tx = self.pool.begin().await?;
        let updated = self.api_key.update(&mut tx, api_key).await?;
        tx.commit().await?;
        Ok(updated)
    }

    async fn revoke_api_key(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.api_key.revoke_by_id(&mut tx, id).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_api_key(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.api_key.delete_by_id(&mut tx, id).await?;
        tx.commit().await?;
        Ok(())
    }

    // ── Admin Keys ──────────────────────────────────────────────────────────

    async fn create_admin_key(
        &self,
        key_hash: &str,
        label: Option<String>,
        permissions: Vec<String>,
    ) -> Result<AdminKey> {
        let mut tx = self.pool.begin().await?;
        let admin_key = self
            .admin_key
            .create(&mut tx, key_hash, label, permissions)
            .await?;
        tx.commit().await?;
        Ok(admin_key)
    }

    async fn get_admin_key_by_hash(&self, key_hash: &str) -> Result<Option<AdminKey>> {
        self.admin_key.get_by_hash(&self.pool, key_hash).await
    }

    async fn get_admin_key_by_id(&self, id: &str) -> Result<Option<AdminKey>> {
        self.admin_key.get_by_id(&self.pool, id).await
    }

    async fn get_all_admin_keys(&self, limit: i64, offset: i64) -> Result<Vec<AdminKey>> {
        self.admin_key.get_all(&self.pool, limit, offset).await
    }

    async fn count_admin_keys(&self) -> Result<i64> {
        self.admin_key.count(&self.pool).await
    }

    async fn update_admin_key(&self, admin_key: AdminKey) -> Result<AdminKey> {
        let mut tx = self.pool.begin().await?;
        let updated = self.admin_key.update(&mut tx, admin_key).await?;
        tx.commit().await?;
        Ok(updated)
    }

    async fn revoke_admin_key(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.admin_key.revoke_by_id(&mut tx, id).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_admin_key(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.admin_key.delete_by_id(&mut tx, id).await?;
        tx.commit().await?;
        Ok(())
    }

    // ── Service Accounts ────────────────────────────────────────────────────

    async fn create_service_account(
        &self,
        name: &str,
        description: Option<&str>,
        org_id: Option<&str>,
    ) -> Result<ServiceAccount> {
        let mut tx = self.pool.begin().await?;
        let sa = self
            .service_account
            .create(&mut tx, name, description, org_id)
            .await?;
        tx.commit().await?;
        Ok(sa)
    }

    async fn get_service_account_by_id(&self, id: &str) -> Result<Option<ServiceAccount>> {
        self.service_account.get_by_id(&self.pool, id).await
    }

    async fn get_all_service_accounts(
        &self,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ServiceAccount>> {
        self.service_account
            .get_all(&self.pool, limit, offset)
            .await
    }

    async fn count_service_accounts(&self) -> Result<i64> {
        self.service_account.count(&self.pool).await
    }

    async fn search_service_accounts(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ServiceAccount>> {
        self.service_account
            .search(&self.pool, query, limit, offset)
            .await
    }

    async fn count_search_service_accounts(&self, query: &str) -> Result<i64> {
        self.service_account.count_search(&self.pool, query).await
    }

    async fn update_service_account(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        org_id: Option<&str>,
    ) -> Result<ServiceAccount> {
        let mut tx = self.pool.begin().await?;
        let sa = self
            .service_account
            .update(&mut tx, id, name, description, org_id)
            .await?;
        tx.commit().await?;
        Ok(sa)
    }

    async fn delete_service_account(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.service_account.delete(&mut tx, id).await?;
        tx.commit().await?;
        Ok(())
    }

    // ── Organizations ───────────────────────────────────────────────────────

    async fn create_organization(
        &self,
        name: &str,
        description: Option<&str>,
        attrs: Option<&serde_json::Value>,
    ) -> Result<Organization> {
        let mut tx = self.pool.begin().await?;
        let org = self
            .organization
            .create(&mut tx, name, description, attrs)
            .await?;
        tx.commit().await?;
        Ok(org)
    }

    async fn get_organization_by_id(&self, id: &str) -> Result<Option<Organization>> {
        self.organization.get_by_id(&self.pool, id).await
    }

    async fn get_all_organizations(&self, limit: i64, offset: i64) -> Result<Vec<Organization>> {
        self.organization.get_all(&self.pool, limit, offset).await
    }

    async fn count_organizations(&self) -> Result<i64> {
        self.organization.count(&self.pool).await
    }

    async fn search_organizations(
        &self,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Organization>> {
        self.organization
            .search(&self.pool, query, limit, offset)
            .await
    }

    async fn count_search_organizations(&self, query: &str) -> Result<i64> {
        self.organization.count_search(&self.pool, query).await
    }

    async fn update_organization(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        attrs: Option<&serde_json::Value>,
    ) -> Result<Organization> {
        let mut tx = self.pool.begin().await?;
        let org = self
            .organization
            .update(&mut tx, id, name, description, attrs)
            .await?;
        tx.commit().await?;
        Ok(org)
    }

    async fn delete_organization(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.organization.delete(&mut tx, id).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn add_user_to_organization(&self, user_id: &str, org_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.organization.add_user(&mut tx, user_id, org_id).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn remove_user_from_organization(&self, user_id: &str, org_id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.organization
            .remove_user(&mut tx, user_id, org_id)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn get_organization_users(&self, org_id: &str) -> Result<Vec<User>> {
        self.organization.get_users(&self.pool, org_id).await
    }

    async fn get_organization_users_paginated(
        &self,
        org_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<User>> {
        self.organization
            .get_users_paginated(&self.pool, org_id, limit, offset)
            .await
    }

    async fn count_organization_users(&self, org_id: &str) -> Result<i64> {
        self.organization.count_users(&self.pool, org_id).await
    }

    async fn get_user_organizations(&self, user_id: &str) -> Result<Vec<Organization>> {
        self.organization
            .get_orgs_by_user(&self.pool, user_id)
            .await
    }

    // ── Audit ───────────────────────────────────────────────────────────────

    async fn insert_audit_log_bulk(&self, logs: Vec<crate::ent::Audit>) -> Result<()> {
        if logs.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;
        self.audit.insert_bulk(&mut tx, &logs).await?;
        tx.commit().await?;
        Ok(())
    }
}
