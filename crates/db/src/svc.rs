use super::repo::{
    ActionRepository, ApiKeyRepository, CredentialRepository, SubjectRepository, UserRepository,
};
use crate::ent::{
    Action, ActionType, ApiKey, Credential, CredentialType, OwnerType, Subject, SubjectType, User,
};
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
    user: UserRepository,
    credential: CredentialRepository,
    subject: SubjectRepository,
    action: ActionRepository,
    api_key: ApiKeyRepository,
}

impl Service {
    #[cfg(feature = "postgres")]
    pub fn new(pool: sqlx::PgPool) -> Self {
        Self {
            pool,
            user: UserRepository::new(),
            credential: CredentialRepository::new(),
            subject: SubjectRepository::new(),
            action: ActionRepository::new(),
            api_key: ApiKeyRepository::new(),
        }
    }

    #[cfg(feature = "sqlite")]
    pub fn new(pool: sqlx::SqlitePool) -> Self {
        Self {
            pool,
            user: UserRepository::new(),
            credential: CredentialRepository::new(),
            subject: SubjectRepository::new(),
            action: ActionRepository::new(),
            api_key: ApiKeyRepository::new(),
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
    ///
    /// # Arguments
    /// * `user`: The user to be created.
    ///
    /// * `credential_type`: The type of credential to be associated with the user.
    ///
    /// * `value`: The hashed password.
    ///
    /// * `attrs`: Optional attributes associated with the user.
    async fn create_user(
        &self,
        user: User,
        credential_type: CredentialType,
        value: &str,
        attrs: Option<JsonValue>,
    ) -> Result<User> {
        let mut tx = self.pool.begin().await?;
        let user_id = user.id.clone();
        let record = self.user.create(&mut tx, user).await?;
        self.credential
            .create(&mut tx, &user_id, credential_type, value)
            .await?;
        self.subject
            .create(&mut tx, SubjectType::User, &user_id, attrs)
            .await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let mut tx = self.pool.begin().await?;
        let user = self.user.get_by_username(&mut tx, username).await?;
        tx.commit().await?;
        Ok(user)
    }

    async fn update_user(&self, user: User) -> Result<User> {
        let mut tx = self.pool.begin().await?;
        let updated_user = self.user.update(&mut tx, user).await?;
        tx.commit().await?;
        Ok(updated_user)
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
        let mut tx = self.pool.begin().await?;
        let credential = self
            .credential
            .get_by_user_id(&mut tx, user_id, credential_type)
            .await?;
        tx.commit().await?;
        Ok(credential)
    }

    async fn create_action(&self, action: Action) -> Result<Action> {
        let mut tx = self.pool.begin().await?;
        let record = self.action.create(&mut tx, action).await?;
        tx.commit().await?;
        Ok(record)
    }

    async fn get_action_by_value(&self, value: &str) -> Result<Option<Action>> {
        let mut tx = self.pool.begin().await?;
        let action = self.action.get_by_value(&mut tx, value).await?;
        tx.commit().await?;
        Ok(action)
    }

    async fn get_action_by_sub_and_type(
        &self,
        sub: &str,
        action_type: ActionType,
    ) -> Result<Option<Action>> {
        let mut tx = self.pool.begin().await?;
        let action = self
            .action
            .get_by_sub_and_type(&mut tx, sub, action_type)
            .await?;
        tx.commit().await?;
        Ok(action)
    }

    async fn get_subject_by_id(&self, subject_id: &str) -> Result<Option<Subject>> {
        let mut tx = self.pool.begin().await?;
        let subject = self.subject.get_by_sub_id(&mut tx, subject_id).await?;
        tx.commit().await?;
        Ok(subject)
    }

    async fn create_api_key(
        &self,
        owner: &str,
        owner_type: OwnerType,
        key_hash: &str,
        label: Option<String>,
        attrs: Option<JsonValue>,
        exp: Option<i64>,
    ) -> Result<ApiKey> {
        let mut tx = self.pool.begin().await?;
        let api_key = self
            .api_key
            .create(&mut tx, owner, owner_type, key_hash, label, exp)
            .await?;

        self.subject
            .create(&mut tx, SubjectType::ApiKey, &api_key.id, attrs)
            .await?;

        tx.commit().await?;
        Ok(api_key)
    }

    async fn get_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>> {
        let mut tx = self.pool.begin().await?;
        let api_key = self.api_key.get_by_hash(&mut tx, key_hash).await?;
        tx.commit().await?;
        Ok(api_key)
    }

    async fn revoke_api_key(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        self.api_key.revoke_by_id(&mut tx, id).await?;
        tx.commit().await?;
        Ok(())
    }
}
