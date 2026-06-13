use super::repo::{
    AdminKeyRepository, ApiKeyRepository, AuditRepository, CredentialRepository,
    OAuthClientRepository, OAuthConsentRepository, OrganizationRepository,
    ServiceAccountRepository, SuperAdminRepository, UserRepository,
};
use crate::backend::Pool;
use crate::ent::{
    ActionType, AdminKey, ApiKey, AuditContext, Credential, CredentialType, OAuthClient,
    OAuthConsent, Organization, Profile, ServiceAccount, SuperAdmin, User,
};
use crate::repo::{
    ADMIN_KEY, API_KEY, CREDENTIAL, OAUTH_CLIENT, ORGANIZATION, SERVICE_ACCOUNT, SUPER_ADMIN, USER,
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
        if let Err(e) = sqlx::raw_sql(sqlx::AssertSqlSafe(ddl))
            .execute(&self.pool)
            .await
        {
            eprintln!("Failed to execute schema from '{}': {}", file, e);
        }
    }

    /// Lightweight connectivity check — executes `SELECT 1`.
    pub async fn ping(&self) -> Result<()> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    /// Record an audit event that is not tied to an entity mutation, e.g. a
    /// login or logout. The caller fully populates `ctx` (resource, actor,
    /// metadata) before calling. The row is written in its own transaction and,
    /// in cluster deployments, picked up by the audit relay like any other
    /// outbox row.
    pub async fn record_audit(&self, ctx: AuditContext, action: ActionType) -> Result<()> {
        let audit = ctx.build_audit(action);
        let mut tx = self.pool.begin().await?;
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(())
    }
}

/// A batch of audit outbox rows claimed for relaying, holding the open
/// transaction that locks them via `FOR UPDATE SKIP LOCKED`.
///
/// Dropping the claim without calling [`AuditClaim::commit_published`] rolls the
/// transaction back and releases the rows, so a later attempt — this node or
/// another — can re-claim them. That is exactly the desired behaviour when the
/// broker publish fails: nothing is marked published, nothing is lost.
#[cfg(feature = "postgres")]
pub struct AuditClaim {
    tx: crate::backend::Tx<'static>,
    rows: Vec<crate::ent::OutboxAudit>,
}

#[cfg(feature = "postgres")]
impl AuditClaim {
    /// The claimed rows, ordered by ascending `seq`.
    pub fn rows(&self) -> &[crate::ent::OutboxAudit] {
        &self.rows
    }

    /// Whether no unpublished rows were available to claim.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Mark every claimed row as published and commit the transaction. Call this
    /// only once the rows have been durably handed to the broker.
    pub async fn commit_published(mut self) -> Result<()> {
        let seqs: Vec<i64> = self.rows.iter().map(|row| row.seq).collect();
        AuditRepository::new()
            .mark_published(&mut self.tx, &seqs)
            .await?;
        self.tx.commit().await?;
        Ok(())
    }
}

#[cfg(feature = "postgres")]
impl Service {
    /// Claim up to `limit` unpublished audit outbox rows for relaying.
    ///
    /// The returned [`AuditClaim`] owns an open transaction that locks the rows;
    /// ship them to the broker, then call [`AuditClaim::commit_published`], or
    /// drop the claim to release them.
    pub async fn claim_unpublished_audits(&self, limit: i64) -> Result<AuditClaim> {
        let mut tx = self.pool.begin().await?;
        let rows = self.audit.claim_unpublished(&mut tx, limit).await?;
        Ok(AuditClaim { tx, rows })
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
        match sqlx::query(sqlx::AssertSqlSafe(stmt)).execute(&pool).await {
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
        ctx: AuditContext,
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
        let audit = ctx
            .with_resource(USER.to_string())
            .with_resource_id(record.id.clone())
            .with_metadata(serde_json::to_value(&record).unwrap_or_default())
            .build_audit(ActionType::Create);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn create_super_admin_user(
        &self,
        profile: Profile,
        credential_type: CredentialType,
        value: &str,
        ctx: AuditContext,
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
        let user_audit = ctx
            .clone()
            .with_resource(USER.to_string())
            .with_resource_id(record.id.clone())
            .with_metadata(serde_json::to_value(&record).unwrap_or_default())
            .build_audit(ActionType::Create);
        let super_admin_audit = ctx
            .with_resource(SUPER_ADMIN.to_string())
            .with_resource_id(record.id.clone())
            .build_audit(ActionType::Create);
        self.audit
            .insert_bulk(&mut tx, &[user_audit, super_admin_audit])
            .await?;
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

    async fn update_user(&self, user: User, ctx: AuditContext) -> Result<User> {
        let mut tx = self.pool.begin().await?;
        let updated_user = self.user.update(&mut tx, user).await?;
        let audit = ctx
            .with_resource(USER.to_string())
            .with_resource_id(updated_user.id.clone())
            .with_metadata(serde_json::to_value(&updated_user).unwrap_or_default())
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(updated_user)
    }

    async fn delete_user(&self, id: &str, ctx: AuditContext) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.user.delete(&mut tx, id).await?;
        let audit = ctx
            .with_resource(USER.to_string())
            .with_resource_id(id.to_string())
            .build_audit(ActionType::Delete);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn change_password(
        &self,
        user_id: &str,
        new_password: &str,
        ctx: AuditContext,
    ) -> Result<Credential> {
        let mut tx = self.pool.begin().await?;
        let credential = self
            .credential
            .change_password(&mut tx, user_id, new_password)
            .await?;
        let audit = ctx
            .with_resource(CREDENTIAL.to_string())
            .with_resource_id(user_id.to_string())
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
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

    async fn create_oauth_client(
        &self,
        client: OAuthClient,
        ctx: AuditContext,
    ) -> Result<OAuthClient> {
        let mut tx = self.pool.begin().await?;
        let client = self.oauth_client.create(&mut tx, client).await?;
        let audit = ctx
            .with_resource(OAUTH_CLIENT.to_string())
            .with_resource_id(client.client_id.clone())
            .with_metadata(serde_json::to_value(&client).unwrap_or_default())
            .build_audit(ActionType::Create);
        self.audit.insert(&mut tx, &audit).await?;
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

    async fn update_oauth_client(
        &self,
        client: OAuthClient,
        ctx: AuditContext,
    ) -> Result<OAuthClient> {
        let mut tx = self.pool.begin().await?;
        let client = self.oauth_client.update(&mut tx, client).await?;
        let audit = ctx
            .with_resource(OAUTH_CLIENT.to_string())
            .with_resource_id(client.client_id.clone())
            .with_metadata(serde_json::to_value(&client).unwrap_or_default())
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(client)
    }

    async fn update_oauth_client_secret_hash(
        &self,
        client_id: &str,
        client_secret_hash: Option<&str>,
        ctx: AuditContext,
    ) -> Result<OAuthClient> {
        let mut tx = self.pool.begin().await?;
        let client = self
            .oauth_client
            .update_secret_hash(&mut tx, client_id, client_secret_hash)
            .await?;
        let audit = ctx
            .with_resource(OAUTH_CLIENT.to_string())
            .with_resource_id(client.client_id.clone())
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(client)
    }

    async fn set_oauth_client_enabled(
        &self,
        client_id: &str,
        enabled: bool,
        ctx: AuditContext,
    ) -> Result<OAuthClient> {
        let mut tx = self.pool.begin().await?;
        let client = self
            .oauth_client
            .set_enabled(&mut tx, client_id, enabled)
            .await?;
        let audit = ctx
            .with_resource(OAUTH_CLIENT.to_string())
            .with_resource_id(client.client_id.clone())
            .with_metadata(serde_json::json!({ "enabled": enabled }))
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(client)
    }

    async fn delete_oauth_client(&self, client_id: &str, ctx: AuditContext) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.oauth_client
            .delete_by_client_id(&mut tx, client_id)
            .await?;
        let audit = ctx
            .with_resource(OAUTH_CLIENT.to_string())
            .with_resource_id(client_id.to_string())
            .build_audit(ActionType::Delete);
        self.audit.insert(&mut tx, &audit).await?;
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
        ctx: AuditContext,
    ) -> Result<ApiKey> {
        let mut tx = self.pool.begin().await?;
        let api_key = self.api_key.create(&mut tx, key_hash, label, attrs).await?;
        self.api_key
            .link_to_user(&mut tx, &api_key.id, user_id)
            .await?;
        let audit = ctx
            .with_resource(API_KEY.to_string())
            .with_resource_id(api_key.id.clone())
            .build_audit(ActionType::Create);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(api_key)
    }

    async fn create_service_account_api_key(
        &self,
        service_account_id: &str,
        key_hash: &str,
        label: &str,
        attrs: Option<JsonValue>,
        ctx: AuditContext,
    ) -> Result<ApiKey> {
        let mut tx = self.pool.begin().await?;
        let api_key = self.api_key.create(&mut tx, key_hash, label, attrs).await?;
        self.api_key
            .link_to_service_account(&mut tx, &api_key.id, service_account_id)
            .await?;
        let audit = ctx
            .with_resource(API_KEY.to_string())
            .with_resource_id(api_key.id.clone())
            .build_audit(ActionType::Create);
        self.audit.insert(&mut tx, &audit).await?;
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

    async fn update_api_key(&self, api_key: ApiKey, ctx: AuditContext) -> Result<ApiKey> {
        let mut tx = self.pool.begin().await?;
        let updated = self.api_key.update(&mut tx, api_key).await?;
        let audit = ctx
            .with_resource(API_KEY.to_string())
            .with_resource_id(updated.id.clone())
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(updated)
    }

    async fn revoke_api_key(&self, id: &str, ctx: AuditContext) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.api_key.revoke_by_id(&mut tx, id).await?;
        let audit = ctx
            .with_resource(API_KEY.to_string())
            .with_resource_id(id.to_string())
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_api_key(&self, id: &str, ctx: AuditContext) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.api_key.delete_by_id(&mut tx, id).await?;
        let audit = ctx
            .with_resource(API_KEY.to_string())
            .with_resource_id(id.to_string())
            .build_audit(ActionType::Delete);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(())
    }

    // ── Admin Keys ──────────────────────────────────────────────────────────

    async fn create_admin_key(
        &self,
        key_hash: &str,
        label: Option<String>,
        permissions: Vec<String>,
        ctx: AuditContext,
    ) -> Result<AdminKey> {
        let mut tx = self.pool.begin().await?;
        let admin_key = self
            .admin_key
            .create(&mut tx, key_hash, label, permissions)
            .await?;
        let audit = ctx
            .with_resource(ADMIN_KEY.to_string())
            .with_resource_id(admin_key.id.clone())
            .build_audit(ActionType::Create);
        self.audit.insert(&mut tx, &audit).await?;
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

    async fn update_admin_key(&self, admin_key: AdminKey, ctx: AuditContext) -> Result<AdminKey> {
        let mut tx = self.pool.begin().await?;
        let updated = self.admin_key.update(&mut tx, admin_key).await?;
        let audit = ctx
            .with_resource(ADMIN_KEY.to_string())
            .with_resource_id(updated.id.clone())
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(updated)
    }

    async fn revoke_admin_key(&self, id: &str, ctx: AuditContext) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.admin_key.revoke_by_id(&mut tx, id).await?;
        let audit = ctx
            .with_resource(ADMIN_KEY.to_string())
            .with_resource_id(id.to_string())
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn delete_admin_key(&self, id: &str, ctx: AuditContext) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.admin_key.delete_by_id(&mut tx, id).await?;
        let audit = ctx
            .with_resource(ADMIN_KEY.to_string())
            .with_resource_id(id.to_string())
            .build_audit(ActionType::Delete);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(())
    }

    // ── Service Accounts ────────────────────────────────────────────────────

    async fn create_service_account(
        &self,
        name: &str,
        description: Option<&str>,
        org_id: Option<&str>,
        ctx: AuditContext,
    ) -> Result<ServiceAccount> {
        let mut tx = self.pool.begin().await?;
        let sa = self
            .service_account
            .create(&mut tx, name, description, org_id)
            .await?;
        let audit = ctx
            .with_resource(SERVICE_ACCOUNT.to_string())
            .with_resource_id(sa.id.clone())
            .with_metadata(serde_json::to_value(&sa).unwrap_or_default())
            .build_audit(ActionType::Create);
        self.audit.insert(&mut tx, &audit).await?;
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
        ctx: AuditContext,
    ) -> Result<ServiceAccount> {
        let mut tx = self.pool.begin().await?;
        let sa = self
            .service_account
            .update(&mut tx, id, name, description, org_id)
            .await?;
        let audit = ctx
            .with_resource(SERVICE_ACCOUNT.to_string())
            .with_resource_id(sa.id.clone())
            .with_metadata(serde_json::to_value(&sa).unwrap_or_default())
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(sa)
    }

    async fn delete_service_account(&self, id: &str, ctx: AuditContext) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.service_account.delete(&mut tx, id).await?;
        let audit = ctx
            .with_resource(SERVICE_ACCOUNT.to_string())
            .with_resource_id(id.to_string())
            .build_audit(ActionType::Delete);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(())
    }

    // ── Organizations ───────────────────────────────────────────────────────

    async fn create_organization(
        &self,
        name: &str,
        description: Option<&str>,
        attrs: Option<&serde_json::Value>,
        ctx: AuditContext,
    ) -> Result<Organization> {
        let mut tx = self.pool.begin().await?;
        let org = self
            .organization
            .create(&mut tx, name, description, attrs)
            .await?;
        let audit = ctx
            .with_resource(ORGANIZATION.to_string())
            .with_resource_id(org.id.clone())
            .with_metadata(serde_json::to_value(&org).unwrap_or_default())
            .build_audit(ActionType::Create);
        self.audit.insert(&mut tx, &audit).await?;
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
        ctx: AuditContext,
    ) -> Result<Organization> {
        let mut tx = self.pool.begin().await?;
        let org = self
            .organization
            .update(&mut tx, id, name, description, attrs)
            .await?;
        let audit = ctx
            .with_resource(ORGANIZATION.to_string())
            .with_resource_id(org.id.clone())
            .with_metadata(serde_json::to_value(&org).unwrap_or_default())
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(org)
    }

    async fn delete_organization(&self, id: &str, ctx: AuditContext) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.organization.delete(&mut tx, id).await?;
        let audit = ctx
            .with_resource(ORGANIZATION.to_string())
            .with_resource_id(id.to_string())
            .build_audit(ActionType::Delete);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn add_user_to_organization(
        &self,
        user_id: &str,
        org_id: &str,
        ctx: AuditContext,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.organization.add_user(&mut tx, user_id, org_id).await?;
        let audit = ctx
            .with_resource(ORGANIZATION.to_string())
            .with_resource_id(org_id.to_string())
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn remove_user_from_organization(
        &self,
        user_id: &str,
        org_id: &str,
        ctx: AuditContext,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.organization
            .remove_user(&mut tx, user_id, org_id)
            .await?;
        let audit = ctx
            .with_resource(ORGANIZATION.to_string())
            .with_resource_id(org_id.to_string())
            .build_audit(ActionType::Update);
        self.audit.insert(&mut tx, &audit).await?;
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
}

// ── Integration tests ──────────────────────────────────────────────────────────
//
// These tests require a live PostgreSQL database. Skip them in CI environments
// that lack one, or run them explicitly:
//
//   cargo test -p db --features postgres -- --ignored
//
// The tests use a unique `request_id` per invocation (ULID) as an isolation key
// so they can run against a shared database without interfering with each other.
#[cfg(all(test, feature = "postgres"))]
mod tests {
    use super::*;
    use crate::ent::{ActionType, AuditContext, CredentialType, Profile};
    use crate::tx::Transaction;

    async fn test_service() -> Service {
        dotenvy::dotenv().ok();
        let url = std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .expect("TEST_DATABASE_URL or DATABASE_URL must be set for postgres tests");
        init(&url).await.expect("DB init failed")
    }

    async fn count_by_request_id(svc: &Service, req_id: &str) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*)::bigint FROM audits WHERE request_id = $1")
            .bind(req_id)
            .fetch_one(&svc.pool)
            .await
            .unwrap_or(0)
    }

    fn unique_email() -> String {
        format!("test-{}@example.com", ulid::Ulid::new())
    }

    fn unique_nick() -> String {
        ulid::Ulid::new().to_string()
    }

    // ── Atomicity ────────────────────────────────────────────────────────────

    /// A rolled-back transaction must not leave any audit row behind.
    #[tokio::test]
    #[ignore = "requires postgres (run with: cargo test -p db --features postgres -- --ignored)"]
    async fn rollback_leaves_no_audit_row() {
        let svc = test_service().await;

        let email = unique_email();
        let req_ok = ulid::Ulid::new().to_string();
        let req_fail = ulid::Ulid::new().to_string();

        // First create succeeds — establishes the email that will trigger a constraint violation.
        let ctx_ok = AuditContext::system().with_request_id(req_ok.clone());
        svc.create_user(
            Profile::new(email.clone(), unique_nick()),
            CredentialType::Password,
            "pw",
            ctx_ok,
        )
        .await
        .expect("first create should succeed");

        // Second create with the same email — unique constraint fires, tx rolls back.
        let ctx_fail = AuditContext::system().with_request_id(req_fail.clone());
        let result = svc
            .create_user(
                Profile::new(email.clone(), unique_nick()),
                CredentialType::Password,
                "pw",
                ctx_fail,
            )
            .await;
        assert!(result.is_err(), "duplicate email should fail");

        // The failed transaction must have written zero audit rows.
        let count = count_by_request_id(&svc, &req_fail).await;
        assert_eq!(count, 0, "rolled-back tx must not leak an audit row");
    }

    /// `create_super_admin_user` writes exactly two audit rows (user + super_admin)
    /// inside a single transaction.
    #[tokio::test]
    #[ignore = "requires postgres (run with: cargo test -p db --features postgres -- --ignored)"]
    async fn super_admin_create_writes_two_audit_rows() {
        let svc = test_service().await;

        let req_id = ulid::Ulid::new().to_string();
        let ctx = AuditContext::system().with_request_id(req_id.clone());

        svc.create_super_admin_user(
            Profile::new(unique_email(), unique_nick()),
            CredentialType::Password,
            "pw",
            ctx,
        )
        .await
        .expect("super_admin create should succeed");

        let count = count_by_request_id(&svc, &req_id).await;
        assert_eq!(count, 2, "create_super_admin_user must write exactly 2 audit rows");
    }

    // ── Relay claim / publish / mark cycle ───────────────────────────────────

    /// Serializes the claim-based tests: cargo runs tests in parallel, and two
    /// concurrent claimers would steal (and publish) each other's rows, making
    /// presence assertions flaky. Insert-only tests don't need this.
    static CLAIM_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    /// A claimed row is absent from subsequent claims after `commit_published`.
    #[tokio::test]
    #[ignore = "requires postgres (run with: cargo test -p db --features postgres -- --ignored)"]
    async fn relay_claim_publish_mark_cycle() {
        let _guard = CLAIM_LOCK.lock().await;
        let svc = test_service().await;

        let req_id = ulid::Ulid::new().to_string();
        let ctx = AuditContext::system()
            .with_request_id(req_id.clone())
            .with_resource(USER.to_string());
        svc.record_audit(ctx, ActionType::Login)
            .await
            .expect("record_audit failed");

        // Claim with a limit large enough to swallow any backlog left behind by
        // the insert-only tests — our row must be present.
        let claim = svc
            .claim_unpublished_audits(100_000)
            .await
            .expect("first claim failed");
        assert!(
            claim
                .rows()
                .iter()
                .any(|r| r.audit.request_id.as_deref() == Some(req_id.as_str())),
            "the audit row written above must appear in the claim"
        );

        // Mark all claimed rows as published.
        claim.commit_published().await.expect("commit_published failed");

        // A fresh claim must not return the same row (published_at IS NOW set).
        let count: i64 =
            sqlx::query_scalar("SELECT COUNT(*)::bigint FROM audits WHERE request_id = $1 AND published_at IS NULL")
                .bind(&req_id)
                .fetch_one(&svc.pool)
                .await
                .unwrap_or(0);
        assert_eq!(count, 0, "after commit_published the row must not be in the unpublished set");
    }

    /// Two concurrent open claims must not overlap — `FOR UPDATE SKIP LOCKED`
    /// ensures each relay node claims a disjoint set of rows.
    #[tokio::test]
    #[ignore = "requires postgres (run with: cargo test -p db --features postgres -- --ignored)"]
    async fn concurrent_claims_are_disjoint() {
        let _guard = CLAIM_LOCK.lock().await;
        let svc = test_service().await;

        let marker = ulid::Ulid::new().to_string();

        // Insert 4 unpublished rows tagged with a unique marker.
        for _ in 0..4 {
            let ctx = AuditContext::system()
                .with_request_id(marker.clone())
                .with_resource(USER.to_string());
            svc.record_audit(ctx, ActionType::Login)
                .await
                .expect("record_audit failed");
        }

        // Open claim1 (limit 2) — keeps its transaction open.
        let claim1 = svc.claim_unpublished_audits(2).await.expect("claim1 failed");
        // Open claim2 (limit 2) — must skip rows locked by claim1.
        let claim2 = svc.claim_unpublished_audits(2).await.expect("claim2 failed");

        let seqs1: std::collections::HashSet<i64> =
            claim1.rows().iter().map(|r| r.seq).collect();
        let seqs2: std::collections::HashSet<i64> =
            claim2.rows().iter().map(|r| r.seq).collect();

        let overlap: Vec<_> = seqs1.intersection(&seqs2).collect();
        assert!(
            overlap.is_empty(),
            "concurrent claims must not claim the same rows; overlap: {:?}",
            overlap
        );

        // Clean up: mark both batches published so they don't linger.
        claim1.commit_published().await.ok();
        claim2.commit_published().await.ok();
    }
}
