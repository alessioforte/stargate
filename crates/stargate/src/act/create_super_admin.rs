use crate::etc;
use db::ent::{CredentialType, User};
use db::Transaction;

pub const SUPER_ADMIN_NICKNAME: &str = "admin";

pub async fn create_super_admin() {
    let service = etc::db::service();

    let super_admin = service
        .get_user_by_username(SUPER_ADMIN_NICKNAME)
        .await
        .unwrap();

    if super_admin.is_some() {
        log::info!("Super admin already exists");
        return;
    }

    let email =
        std::env::var("SUPER_ADMIN_EMAIL").unwrap_or_else(|_| "admin@localhost".to_string());
    let name = std::env::var("SUPER_ADMIN_NAME").unwrap_or_else(|_| "Admin".to_string());
    let pw = password::generator(40, true, true, true, false);
    let user = User::new(email)
        .first_name(Some(name))
        .nickname(Some(SUPER_ADMIN_NICKNAME.to_string()))
        .phone_number(None)
        .picture(None);

    match service
        .create_user(
            user,
            CredentialType::Password,
            &password::Hash::encode(&pw).unwrap(),
        )
        .await
    {
        Ok(_) => log::info!("Super admin password: {}", pw),
        Err(e) => log::error!("Failed to create super admin: {}", e),
    };
}
