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
    profile: Profile,
    credential_type: CredentialType,
    value: &str,
    ctx: AuditContext,
) -> Result<User> {
    let svc = service();
    let metadata = serde_json::to_value(&profile).unwrap_or_default();
    match svc.create_user(profile, credential_type, value).await {
        Ok(user) => {
            let resource = USER.to_string();
            let ctx = ctx.with_resource(resource).with_metadata(metadata);
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

pub async fn get_user_by_id(id: &str) -> Result<Option<User>> {
    let svc = service();
    svc.get_user_by_id(id).await
}

pub async fn get_all_users(limit: i64, offset: i64) -> Result<Vec<User>> {
    let svc = service();
    svc.get_all_users(limit, offset).await
}

pub async fn count_users() -> Result<i64> {
    let svc = service();
    svc.count_users().await
}

pub async fn search_users(query: &str, limit: i64, offset: i64) -> Result<Vec<User>> {
    let svc = service();
    svc.search_users(query, limit, offset).await
}

pub async fn count_search_users(query: &str) -> Result<i64> {
    let svc = service();
    svc.count_search_users(query).await
}

pub async fn update_user(user: User, ctx: AuditContext) -> Result<User> {
    let svc = service();
    let metadata = serde_json::to_value(&user).unwrap_or_default();
    match svc.update_user(user).await {
        Ok(user) => {
            let resource = USER.to_string();
            let ctx = ctx.with_resource(resource).with_metadata(metadata);
            audit::modification!(ctx);
            Ok(user)
        }
        Err(e) => {
            tracing::error!("Error updating user: {:?}", e);
            Err(e)
        }
    }
}

pub async fn delete_user(id: &str, ctx: AuditContext) -> Result<()> {
    let svc = service();
    let metadata = serde_json::to_value(&id).unwrap_or_default();
    match svc.delete_user(id).await {
        Ok(()) => {
            let resource = USER.to_string();
            let ctx = ctx.with_resource(resource).with_metadata(metadata);
            audit::deletion!(ctx);
            Ok(())
        }
        Err(e) => {
            tracing::error!("Error deleting user: {:?}", e);
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
    let metadata = serde_json::to_value(&user_id).unwrap_or_default();
    match svc.change_password(user_id, new_password).await {
        Ok(credential) => {
            let resource = CREDENTIAL.to_string();
            let ctx = ctx.with_resource(resource).with_metadata(metadata);
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
    let metadata = serde_json::to_value(&account_id).unwrap_or_default();
    match svc.create_api_key(account_id, key_hash, label, attrs).await {
        Ok(api_key) => {
            let resource = API_KEY.to_string();
            let ctx = ctx.with_resource(resource).with_metadata(metadata);
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
