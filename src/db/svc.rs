use super::service;
use crate::aud::audit;

use anyhow::Result;
use db::{
    Transaction,
    ent::{ActionType, ApiKey, AuditContext, Credential, CredentialType, Profile, User},
    repo::API_KEY,
    repo::CREDENTIAL,
    repo::USER,
};

pub async fn create_user(
    user: Profile,
    credential_type: CredentialType,
    value: &str,
    ctx: AuditContext,
) -> Result<User> {
    let svc = service();
    match svc.create_user(user, credential_type, value).await {
        Ok(user) => {
            let resource = USER.to_string();
            let ctx = ctx.with_resource(resource);
            audit::creation!(ctx);
            Ok(user)
        }
        Err(e) => {
            tracing::error!("Error creating user: {:?}", e);
            Err(e)
        }
    }
}

pub async fn get_user_by_username(username: &str) -> Result<Option<User>> {
    let svc = service();
    svc.get_user_by_username(username).await
}

pub async fn update_user(user: User, ctx: AuditContext) -> Result<User> {
    let svc = service();
    match svc.update_user(user).await {
        Ok(user) => {
            let resource = USER.to_string();
            let ctx = ctx.with_resource(resource);
            audit::modification!(ctx);
            Ok(user)
        }
        Err(e) => {
            tracing::error!("Error updating user: {:?}", e);
            Err(e)
        }
    }
}

pub async fn change_password(
    user_id: &str,
    new_password: &str,
    ctx: AuditContext,
) -> Result<Credential> {
    let svc = service();
    match svc.change_password(user_id, new_password).await {
        Ok(credential) => {
            let resource = CREDENTIAL.to_string();
            let ctx = ctx.with_resource(resource);
            audit::modification!(ctx);
            Ok(credential)
        }
        Err(e) => {
            tracing::error!("Error changing password: {:?}", e);
            Err(e)
        }
    }
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
    ctx: AuditContext,
) -> Result<ApiKey> {
    let svc = service();
    match svc.create_api_key(account_id, key_hash, label, attrs).await {
        Ok(api_key) => {
            let resource = API_KEY.to_string();
            let ctx = ctx.with_resource(resource);
            audit::creation!(ctx);
            Ok(api_key)
        }
        Err(e) => {
            tracing::error!("Error creating API key: {:?}", e);
            Err(e)
        }
    }
}

pub async fn get_api_key_by_hash(key_hash: &str) -> Result<Option<ApiKey>> {
    let svc = service();
    svc.get_api_key_by_hash(key_hash).await
}

// pub async fn revoke_api_key(id: &str) -> Result<()> {
//     let svc = service();
//     svc.revoke_api_key(id).await
// }
