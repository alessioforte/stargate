use crate::ent::AdminKey;
use anyhow::Result;
use chrono::Utc;

pub const ADMIN_KEY: &str = "admin_keys";

#[derive(Clone)]
pub struct AdminKeyRepository {}

impl AdminKeyRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for AdminKeyRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl AdminKeyRepository {
    pub async fn create(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        key_hash: &str,
        label: Option<String>,
        permissions: Vec<String>,
    ) -> Result<AdminKey> {
        let admin_key = AdminKey::new(key_hash.to_string(), label, permissions);
        let row = sqlx::query_as::<_, AdminKey>(sqlx::AssertSqlSafe(format!(
            "
            INSERT INTO {admin_keys} (id, key_hash, label, permissions, revoked, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
        ",
            admin_keys = ADMIN_KEY
        )))
        .bind(&admin_key.id)
        .bind(&admin_key.key_hash)
        .bind(&admin_key.label)
        .bind(&admin_key.permissions)
        .bind(admin_key.revoked)
        .bind(admin_key.created_at)
        .bind(admin_key.updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_hash<'c, E>(&self, ex: E, key_hash: &str) -> Result<Option<AdminKey>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, AdminKey>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {admin_keys} WHERE key_hash = $1
        ",
            admin_keys = ADMIN_KEY
        )))
        .bind(key_hash)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn get_by_id<'c, E>(&self, ex: E, id: &str) -> Result<Option<AdminKey>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, AdminKey>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {admin_keys} WHERE id = $1
        ",
            admin_keys = ADMIN_KEY
        )))
        .bind(id)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn get_by_id_for_update(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        id: &str,
    ) -> Result<Option<AdminKey>> {
        let row = sqlx::query_as::<_, AdminKey>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {admin_keys} WHERE id = $1 {for_update}",
            admin_keys = ADMIN_KEY,
            for_update = crate::backend::FOR_UPDATE,
        )))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_all<'c, E>(&self, ex: E, limit: i64, offset: i64) -> Result<Vec<AdminKey>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, AdminKey>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {admin_keys} ORDER BY created_at DESC, id DESC LIMIT $1 OFFSET $2
        ",
            admin_keys = ADMIN_KEY
        )))
        .bind(limit)
        .bind(offset)
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }

    pub async fn count<'c, E>(&self, ex: E) -> Result<i64>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "
            SELECT COUNT(*) FROM {admin_keys}
        ",
            admin_keys = ADMIN_KEY
        )))
        .fetch_one(ex)
        .await?;

        Ok(row.0)
    }

    pub async fn update(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        admin_key: AdminKey,
    ) -> Result<AdminKey> {
        let updated_at = Utc::now();
        let row = sqlx::query_as::<_, AdminKey>(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {admin_keys}
            SET label = $2, permissions = $3, updated_at = $4
            WHERE id = $1
            RETURNING *
        ",
            admin_keys = ADMIN_KEY
        )))
        .bind(&admin_key.id)
        .bind(&admin_key.label)
        .bind(&admin_key.permissions)
        .bind(updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn revoke_by_id(&self, tx: &mut crate::backend::Tx<'_>, id: &str) -> Result<()> {
        let updated_at = Utc::now();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {admin_keys} SET revoked = TRUE, updated_at = $2 WHERE id = $1
        ",
            admin_keys = ADMIN_KEY
        )))
        .bind(id)
        .bind(updated_at)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn delete_by_id(&self, tx: &mut crate::backend::Tx<'_>, id: &str) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "
            DELETE FROM {admin_keys} WHERE id = $1
        ",
            admin_keys = ADMIN_KEY
        )))
        .bind(id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}
