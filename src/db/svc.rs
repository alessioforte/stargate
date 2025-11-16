use super::service;
use anyhow::Result;
use db::{
    Transaction,
    ent::{ApiKey, Credential, CredentialType, Profile, User},
};

pub async fn create_user(
    user: Profile,
    credential_type: CredentialType,
    value: &str,
) -> Result<User> {
    let svc = service();
    match svc.create_user(user, credential_type, value).await {
        Ok(user) => {
            // log audit record here if needed
            Ok(user)
        }
        Err(e) => Err(e),
    }
}

pub async fn get_user_by_username(username: &str) -> Result<Option<User>> {
    let svc = service();
    svc.get_user_by_username(username).await
}

pub async fn update_user(user: User) -> Result<User> {
    let svc = service();
    svc.update_user(user).await
}

pub async fn change_password(user_id: &str, new_password: &str) -> Result<Credential> {
    let svc = service();
    svc.change_password(user_id, new_password).await
}

pub async fn get_credential(
    user_id: &str,
    credential_type: CredentialType,
) -> Result<Option<Credential>> {
    let svc = service();
    svc.get_credential(user_id, credential_type).await
}

pub async fn create_api_key(
    account_id: &str,
    key_hash: &str,
    label: Option<String>,
    attrs: Option<serde_json::Value>,
) -> Result<ApiKey> {
    let svc = service();
    svc.create_api_key(account_id, key_hash, label, attrs).await
}

pub async fn get_api_key_by_hash(key_hash: &str) -> Result<Option<ApiKey>> {
    let svc = service();
    svc.get_api_key_by_hash(key_hash).await
}

// pub async fn revoke_api_key(id: &str) -> Result<()> {
//     let svc = service();
//     svc.revoke_api_key(id).await
// }
