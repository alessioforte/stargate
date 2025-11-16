use crate::etc;
use db::ent::{CredentialType, Profile};
use tracing::{error, info};

pub async fn create_super_admin() {
    let super_admin = crate::db::get_user_by_username(etc::consts::STARGATE_ADMIN)
        .await
        .unwrap();

    if super_admin.is_some() {
        info!("Super admin already exists.");
        return;
    }

    let email =
        std::env::var("SUPER_ADMIN_EMAIL").unwrap_or_else(|_| "admin@localhost".to_string());
    let name = std::env::var("SUPER_ADMIN_NAME").unwrap_or_else(|_| "Admin".to_string());
    let password = pw::generator(40, true, true, true, false);
    let user = Profile::new(email)
        .given_name(Some(name))
        .nickname(Some(etc::consts::STARGATE_ADMIN.to_string()))
        .phone_number(None)
        .picture(None)
        .attrs(serde_json::json!({
            "role": etc::consts::STARGATE_ADMIN,
        }));

    match crate::db::create_user(
        user,
        CredentialType::Password,
        &pw::Hash::encode(&password).unwrap(),
    )
    .await
    {
        Ok(_) => info!("Super admin password: {}", password),
        Err(e) => error!("Failed to create super admin: {}", e),
    };
}
