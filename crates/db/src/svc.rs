use super::repo::{
    AccountRepository, ApiKeyRepository, AuditRepository, CredentialRepository, UserRepository,
};
use crate::ent::{AccountType, ApiKey, Credential, CredentialType, Profile, User};
use crate::tx::Transaction;
use anyhow::Result;
use serde_json::Value as JsonValue;
#[cfg(feature = "postgres")]
use sqlx::PgPool;
use std::fs;

#[derive(Clone)]
pub struct Service {
    #[cfg(feature = "postgres")]
    pool: sqlx::PgPool,
    #[cfg(feature = "sqlite")]
    pool: sqlx::SqlitePool,
    account: AccountRepository,
    user: UserRepository,
    credential: CredentialRepository,
    api_key: ApiKeyRepository,
    audit: AuditRepository,
}

impl Service {
    pub fn new(
        #[cfg(feature = "sqlite")] pool: sqlx::SqlitePool,
        #[cfg(feature = "postgres")] pool: sqlx::PgPool,
    ) -> Self {
        Self {
            pool,
            account: AccountRepository::new(),
            user: UserRepository::new(),
            credential: CredentialRepository::new(),
            api_key: ApiKeyRepository::new(),
            audit: AuditRepository::new(),
        }
    }

    // TODO: handle with migrations
    pub async fn init_schema(&self, file: &str) {
        let ddl = fs::read_to_string(file).expect("Failed to read schema file");
        for stmt in ddl.split(';') {
            if !stmt.trim().is_empty() {
                sqlx::query(stmt)
                    .execute(&self.pool)
                    .await
                    .expect("Failed to execute schema statement");
            }
        }
    }
}

// Generic init function that works with any database URL
pub async fn init(conn: &str) -> Result<Service> {
    #[cfg(feature = "sqlite")]
    let pool = sqlx::SqlitePool::connect(conn).await?;

    #[cfg(feature = "postgres")]
    let pool = PgPool::connect(conn).await?;

    let service = Service::new(pool);

    #[cfg(feature = "postgres")]
    service.init_schema("ddl/postgres.sql").await;

    #[cfg(feature = "sqlite")]
    service.init_schema("ddl/sqlite.sql").await;

    Ok(service)
}

#[async_trait::async_trait]
impl Transaction for Service {
    /// Creates a new user with the specified credential type and value.
    async fn create_user(
        &self,
        profile: Profile,
        credential_type: CredentialType,
        value: &str,
    ) -> Result<User> {
        let mut tx = self.pool.begin().await?;
        let name = format!(
            "{} {}",
            profile.given_name.clone().unwrap_or_default(),
            profile.family_name.clone().unwrap_or_default()
        )
        .trim()
        .to_string();

        let account = self
            .account
            .create(&mut tx, AccountType::User, &name, None)
            .await?;

        let user = User::new(account.id, profile.email)
            .given_name(profile.given_name)
            .family_name(profile.family_name)
            .picture(profile.picture)
            .nickname(profile.nickname)
            .attrs(profile.attrs);

        let user_id = user.id.clone();
        let record = self.user.create(&mut tx, user).await?;
        self.credential
            .create(&mut tx, &user_id, credential_type, value)
            .await?;
        tx.commit().await?;
        Ok(record)
    }

    /// Retrieves a user by their username (email).
    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let mut tx = self.pool.begin().await?;
        let user = self.user.get_by_username(&mut tx, username).await?;
        tx.commit().await?;
        Ok(user)
    }

    /// Retrieves a user by their ID.
    async fn get_user_by_id(&self, id: &str) -> Result<Option<User>> {
        let mut tx = self.pool.begin().await?;
        let user = self.user.get_by_id(&mut tx, id).await?;
        tx.commit().await?;
        Ok(user)
    }

    /// Retrieves all users with pagination.
    async fn get_all_users(&self, limit: i64, offset: i64) -> Result<Vec<User>> {
        let mut tx = self.pool.begin().await?;
        let users = self.user.get_all(&mut tx, limit, offset).await?;
        tx.commit().await?;
        Ok(users)
    }

    /// Counts total number of users.
    async fn count_users(&self) -> Result<i64> {
        let mut tx = self.pool.begin().await?;
        let count = self.user.count(&mut tx).await?;
        tx.commit().await?;
        Ok(count)
    }

    /// Searches users with pagination.
    async fn search_users(&self, query: &str, limit: i64, offset: i64) -> Result<Vec<User>> {
        let mut tx = self.pool.begin().await?;
        let users = self
            .user
            .search_with_pagination(&mut tx, query, limit, offset)
            .await?;
        tx.commit().await?;
        Ok(users)
    }

    /// Counts users matching search query.
    async fn count_search_users(&self, query: &str) -> Result<i64> {
        let mut tx = self.pool.begin().await?;
        let count = self.user.count_search(&mut tx, query).await?;
        tx.commit().await?;
        Ok(count)
    }

    /// Updates the information of an existing user.
    async fn update_user(&self, user: User) -> Result<User> {
        let mut tx = self.pool.begin().await?;
        let updated_user = self.user.update(&mut tx, user).await?;
        let name = format!(
            "{} {}",
            updated_user.given_name.clone().unwrap_or_default(),
            updated_user.family_name.clone().unwrap_or_default()
        )
        .trim()
        .to_string();
        let _ = self
            .account
            .update(
                &mut tx,
                &updated_user.account_id,
                AccountType::User,
                &name,
                None,
            )
            .await?;
        tx.commit().await?;
        Ok(updated_user)
    }

    /// Deletes a user and all associated data (credentials, account).
    async fn delete_user(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // Get user to find account_id
        let user = self.user.get_by_id(&mut tx, id).await?;
        if let Some(user) = user {
            // Delete credentials first (foreign key constraint)
            self.credential.delete_by_user_id(&mut tx, id).await?;
            // Delete user
            self.user.delete(&mut tx, id).await?;
            // Delete account
            self.account.delete(&mut tx, &user.account_id).await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Changes the password for a user.
    async fn change_password(&self, user_id: &str, new_password: &str) -> Result<Credential> {
        let mut tx = self.pool.begin().await?;
        let credential = self
            .credential
            .change_password(&mut tx, user_id, new_password)
            .await?;
        tx.commit().await?;
        Ok(credential)
    }

    /// Retrieves a credential for a user by credential type.
    async fn get_credential(
        &self,
        user_id: &str,
        credential_type: CredentialType,
    ) -> Result<Option<Credential>> {
        let mut tx = self.pool.begin().await?;
        let credential = self
            .credential
            .get_by_user_id(&mut tx, user_id, credential_type)
            .await?;
        tx.commit().await?;
        Ok(credential)
    }

    /// Creates a new API key for the specified account.
    async fn create_api_key(
        &self,
        account_id: &str,
        key_hash: &str,
        label: Option<String>,
        attrs: Option<JsonValue>,
    ) -> Result<ApiKey> {
        let mut tx = self.pool.begin().await?;
        let api_key = self
            .api_key
            .create(&mut tx, account_id, key_hash, label, attrs)
            .await?;

        tx.commit().await?;
        Ok(api_key)
    }

    /// Retrieves an API key by its hash.
    async fn get_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        let mut tx = self.pool.begin().await?;
        let api_key = self.api_key.get_by_hash(&mut tx, key_hash).await?;
        tx.commit().await?;
        Ok(api_key)
    }

    /// Revokes an API key by its ID.
    async fn revoke_api_key(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.api_key.revoke_by_id(&mut tx, id).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Inserts multiple audit logs in bulk.
    async fn insert_audit_log_bulk(&self, logs: Vec<crate::ent::Audit>) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for log in logs {
            self.audit.insert(&mut tx, &log).await?;
        }
        tx.commit().await?;
        Ok(())
    }
}
