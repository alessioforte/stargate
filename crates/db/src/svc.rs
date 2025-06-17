use crate::ent::{Action, ActionType, Credential, CredentialType, User};
use anyhow::Result;

#[async_trait::async_trait]
pub trait Transaction {
    async fn create_user(
        &self,
        user: User,
        credential_type: CredentialType,
        value: &str,
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
}
