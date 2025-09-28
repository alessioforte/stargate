use crate::ent::{
    Action, ActionType, ApiKey, Credential, CredentialType, Limits, OwnerType, Subject, User,
};
use anyhow::Result;
use serde_json::Value as JsonValue;

#[async_trait::async_trait]
pub trait Transaction {
    async fn create_user(
        &self,
        user: User,
        credential_type: CredentialType,
        value: &str,
        attrs: Option<JsonValue>,
        limits: Option<Limits>,
    ) -> Result<User>;
    async fn get_user_by_username(&self, username: &str) -> Result<Option<User>>;
    async fn update_user(&self, user: User) -> Result<User>;
    async fn change_password(&self, user_id: &str, new_password: &str) -> Result<Credential>;
    async fn get_credential(
        &self,
        user_id: &str,
        credential_type: CredentialType,
    ) -> Result<Option<Credential>>;
    async fn create_action(&self, action: Action) -> Result<Action>;
    async fn get_action_by_value(&self, value: &str) -> Result<Option<Action>>;
    async fn get_action_by_sub_and_type(
        &self,
        sub: &str,
        action_type: ActionType,
    ) -> Result<Option<Action>>;
    async fn get_subject_by_id(&self, subject_id: &str) -> Result<Option<Subject>>;
    async fn create_api_key(
        &self,
        owner: &str,
        owner_type: OwnerType,
        key_hash: &str,
        label: Option<String>,
        attrs: Option<JsonValue>,
        limits: Option<Limits>,
        exp: Option<i64>,
    ) -> Result<ApiKey>;
    async fn get_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>>;
    async fn revoke_api_key(&self, id: &str) -> Result<()>;
}
