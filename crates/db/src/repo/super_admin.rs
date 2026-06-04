use crate::ent::SuperAdmin;
use anyhow::Result;

pub const SUPER_ADMIN: &str = "super_admins";

#[derive(Clone)]
pub struct SuperAdminRepository {}

impl SuperAdminRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for SuperAdminRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl SuperAdminRepository {
    pub async fn create(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
    ) -> Result<SuperAdmin> {
        let super_admin = SuperAdmin::new(user_id.to_string());
        let row = sqlx::query_as::<_, SuperAdmin>(
            sqlx::AssertSqlSafe(format!(
                "
            INSERT INTO {super_admins} (user_id, active)
            VALUES ($1, $2)
            RETURNING *
        ",
                super_admins = SUPER_ADMIN
            )),
        )
        .bind(&super_admin.user_id)
        .bind(super_admin.active)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_user_id<'c, E>(&self, ex: E, user_id: &str) -> Result<Option<SuperAdmin>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, SuperAdmin>(
            sqlx::AssertSqlSafe(format!(
                "
            SELECT * FROM {super_admins}
            WHERE user_id = $1 AND active = TRUE
        ",
                super_admins = SUPER_ADMIN
            )),
        )
        .bind(user_id)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn count_active<'c, E>(&self, ex: E) -> Result<i64>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row: (i64,) = sqlx::query_as(
            sqlx::AssertSqlSafe(format!(
                "
            SELECT COUNT(*) FROM {super_admins} WHERE active = TRUE
        ",
                super_admins = SUPER_ADMIN
            )),
        )
        .fetch_one(ex)
        .await?;

        Ok(row.0)
    }
}
