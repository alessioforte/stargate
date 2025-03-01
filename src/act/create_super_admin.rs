use super::generate_password;
use crate::ent::users::User;
use crate::pks::hash::Hash;

pub async fn create_super_admin() {
    let password = generate_password(40, true, true, true, false);

    let super_admin = User::get_by_email("admin@localhost").await.unwrap();
    if super_admin.is_some() {
        log::info!("Super admin already exists");
        return;
    }

    let super_admin = User::new()
        .email("admin@localhost".to_string())
        .name("Admin".to_string())
        .nickname(Some("admin".to_string()))
        .password(Some(Hash::encode(&password).unwrap()))
        .phone_number(None)
        .picture(None);

    super_admin.save().await.unwrap();

    log::info!("Super admin password: {}", password);
}
