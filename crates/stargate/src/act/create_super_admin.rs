use crate::ent::user::User;
use password::{generate_password, Hash};

pub const SUPER_ADMIN_NICKNAME: &str = "admin";

pub async fn create_super_admin() {
    let password = generate_password(40, true, true, true, false);
    let super_admin = User::get_by_nickname(SUPER_ADMIN_NICKNAME).await.unwrap();
    if super_admin.is_some() {
        log::info!("Super admin already exists");
        return;
    }

    let email =
        std::env::var("SUPER_ADMIN_EMAIL").unwrap_or_else(|_| "admin@localhost".to_string());
    let name = std::env::var("SUPER_ADMIN_NAME").unwrap_or_else(|_| "Admin".to_string());

    let super_admin = User::new()
        .email(email)
        .name(name)
        .nickname(Some(SUPER_ADMIN_NICKNAME.to_string()))
        .password(Some(Hash::encode(&password).unwrap()))
        .phone_number(None)
        .picture(None);

    super_admin.save().await.unwrap();

    log::info!("Super admin password: {}", password);
}
