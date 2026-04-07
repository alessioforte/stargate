use crate::etc::consts::STARGATE_ADMIN;
use db::ent::{AuditContext, CredentialType, Profile};
use jwt::Claims;
use tracing::{error, info};

pub async fn create_super_admin() {
    let super_admin = match crate::db::get_user_by_username(STARGATE_ADMIN).await {
        Ok(user) => user,
        Err(e) => {
            error!("Failed to get super admin: {}", e);
            return;
        }
    };

    if super_admin.is_some() {
        info!("Super admin already exists.");
        return;
    }

    let email =
        std::env::var("SUPER_ADMIN_EMAIL").unwrap_or_else(|_| "admin@localhost".to_string());
    let name = std::env::var("SUPER_ADMIN_NAME").unwrap_or_else(|_| "Admin".to_string());
    let password = pw::generator(40, true, true, true, false);
    let user = Profile::new(email, STARGATE_ADMIN.to_string())
        .given_name(Some(name))
        .phone_number(None)
        .picture(None);

    let hash = match pw::Hash::encode(&password) {
        Ok(h) => h,
        Err(e) => {
            error!("Failed to hash super admin password: {}", e);
            return;
        }
    };

    match crate::db::create_user(
        user,
        CredentialType::Password,
        &hash,
        AuditContext::system(),
    )
    .await
    {
        Ok(_) => {
            info!("Super admin created successfully.");
            eprintln!("Super admin password: {}", password);
        }
        Err(e) => error!("Failed to create super admin: {}", e),
    };
}

pub fn check_super_admin_by_claims(claims: &Claims) -> bool {
    if let Some(role) = claims.role.as_ref() {
        if role == STARGATE_ADMIN {
            return true;
        }
    }
    false
}
