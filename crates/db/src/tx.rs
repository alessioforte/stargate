use crate::ent::{ApiKey, Credential, CredentialType, Profile, User};
use anyhow::Result;
use serde_json::Value as JsonValue;

#[async_trait::async_trait]
pub trait Transaction {
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
    async fn create_api_key(
        &self,
        account_id: &str,
        key_hash: &str,
        label: Option<String>,
        attrs: Option<JsonValue>,
    ) -> Result<ApiKey>;
    async fn get_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>>;
    async fn revoke_api_key(&self, id: &str) -> Result<()>;
    async fn insert_audit_log_bulk(&self, logs: Vec<crate::ent::Audit>) -> Result<()>;
}
