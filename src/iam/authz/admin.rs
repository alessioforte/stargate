pub const SUPER_ADMIN_ROLE: &str = "super_admin";

pub async fn is_super_admin_user_id(user_id: &str) -> anyhow::Result<bool> {
    crate::data::db::is_super_admin_user_id(user_id).await
}
