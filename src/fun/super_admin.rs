use anyhow::{Result, bail};
use db::ent::{AuditContext, CredentialType, Profile};
use tracing::info;

pub const SUPER_ADMIN_ROLE: &str = "super_admin";

pub async fn super_admin_exists() -> Result<bool> {
    crate::db::super_admin_exists().await
}

pub async fn is_super_admin_user_id(user_id: &str) -> Result<bool> {
    crate::db::is_super_admin_user_id(user_id).await
}

pub async fn bootstrap_super_admin(
    email: String,
    password: String,
    name: Option<String>,
    nickname: Option<String>,
) -> Result<()> {
    if password.trim().is_empty() {
        bail!("password must not be empty");
    }

    if super_admin_exists().await? {
        bail!("a super admin already exists");
    }

    if crate::db::get_user_by_username(&email).await?.is_some() {
        bail!("user with email '{email}' already exists");
    }

    let hash = crate::etc::pw::hash_password(password)
        .await
        .ok_or_else(|| anyhow::anyhow!("failed to hash bootstrap password"))?;
    let nickname = nickname.unwrap_or_else(|| email.clone());

    let profile = Profile::new(email.clone(), nickname).given_name(name);

    crate::db::create_super_admin_user(
        profile,
        CredentialType::Password,
        &hash,
        AuditContext::system(),
    )
    .await?;

    info!("Super admin bootstrapped for {}", email);
    Ok(())
}
