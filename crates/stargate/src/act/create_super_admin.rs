use crate::etc;
use db::ent::{CredentialType, User};
use db::Transaction;

pub async fn create_super_admin() {
    let service = etc::db::service();

    let super_admin = service
        .get_user_by_username(etc::consts::STARGATE_ADMIN)
        .await
        .unwrap();

    if super_admin.is_some() {
        let sub_id = super_admin.as_ref().unwrap().id.clone();
        let subject = service.get_subject_by_id(&sub_id).await.unwrap();
        if subject.is_some() {
            let subject = subject.unwrap();
            let role = subject.attrs.get("role");
            if role.is_some_and(|v| v == etc::consts::STARGATE_ADMIN) {
                log::info!("Super admin already exists.");
                return;
            }
        } else {
            log::warn!("Super admin subject not found, creating a new one.");
        }
    }

    let email =
        std::env::var("SUPER_ADMIN_EMAIL").unwrap_or_else(|_| "admin@localhost".to_string());
    let name = std::env::var("SUPER_ADMIN_NAME").unwrap_or_else(|_| "Admin".to_string());
    let password = pw::generator(40, true, true, true, false);
    let user = User::new(email)
        .first_name(Some(name))
        .nickname(Some(etc::consts::STARGATE_ADMIN.to_string()))
        .phone_number(None)
        .picture(None);

    let attrs = serde_json::json!({
        "role": etc::consts::STARGATE_ADMIN,
    });

    match service
        .create_user(
            user,
            CredentialType::Password,
            &pw::Hash::encode(&password).unwrap(),
            Some(attrs),
        )
        .await
    {
        Ok(_) => log::info!("Super admin password: {}", password),
        Err(e) => log::error!("Failed to create super admin: {}", e),
    };
}
