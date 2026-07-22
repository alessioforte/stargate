use super::SERVICE_ACCOUNT;
use crate::ent::{ApiKey, ApiKeyAuth};
use anyhow::Result;
use chrono::Utc;

pub const API_KEY: &str = "api_keys";
pub const USER_API_KEY: &str = "user_api_keys";
pub const SERVICE_ACCOUNT_API_KEY: &str = "service_account_api_keys";

#[derive(sqlx::FromRow, Debug, Clone)]
pub struct ApiKeyAuditRecord {
    #[sqlx(flatten)]
    pub api_key: ApiKey,
    pub owner_type: String,
    pub owner_id: String,
    pub org_id: Option<String>,
}

#[derive(Clone)]
pub struct ApiKeyRepository {}

impl ApiKeyRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ApiKeyRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl ApiKeyRepository {
    pub async fn create(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        key_hash: &str,
        label: &str,
        attrs: Option<serde_json::Value>,
    ) -> Result<ApiKey> {
        let api_key = ApiKey::new(
            key_hash.to_string(),
            label.to_string(),
            attrs.unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
        );
        let row = sqlx::query_as::<_, ApiKey>(sqlx::AssertSqlSafe(format!(
            "
            INSERT INTO {api_keys} (id, key_hash, label, revoked, attrs, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING *
        ",
            api_keys = API_KEY
        )))
        .bind(&api_key.id)
        .bind(&api_key.key_hash)
        .bind(&api_key.label)
        .bind(api_key.revoked)
        .bind(&api_key.attrs)
        .bind(api_key.created_at)
        .bind(api_key.updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn link_to_user(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        api_key_id: &str,
        user_id: &str,
        org_id: Option<&str>,
    ) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {tbl} (api_key_id, user_id, org_id) VALUES ($1, $2, $3)",
            tbl = USER_API_KEY
        )))
        .bind(api_key_id)
        .bind(user_id)
        .bind(org_id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn link_to_service_account(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        api_key_id: &str,
        service_account_id: &str,
    ) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {tbl} (api_key_id, service_account_id) VALUES ($1, $2)",
            tbl = SERVICE_ACCOUNT_API_KEY
        )))
        .bind(api_key_id)
        .bind(service_account_id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn get_by_user_id<'c, E>(&self, ex: E, user_id: &str) -> Result<Vec<ApiKey>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, ApiKey>(sqlx::AssertSqlSafe(format!(
            "
            SELECT ak.* FROM {api_keys} ak
            INNER JOIN {user_api_keys} uak ON ak.id = uak.api_key_id
            WHERE uak.user_id = $1
            ORDER BY ak.created_at DESC, ak.id DESC
        ",
            api_keys = API_KEY,
            user_api_keys = USER_API_KEY
        )))
        .bind(user_id)
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }

    pub async fn get_all_by_user_type<'c, E>(
        &self,
        ex: E,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ApiKey>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, ApiKey>(sqlx::AssertSqlSafe(format!(
            "SELECT ak.* FROM {api_keys} ak
                INNER JOIN {user_api_keys} uak ON ak.id = uak.api_key_id
                ORDER BY ak.created_at DESC, ak.id DESC LIMIT $1 OFFSET $2",
            api_keys = API_KEY,
            user_api_keys = USER_API_KEY
        )))
        .bind(limit)
        .bind(offset)
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }

    pub async fn count_by_user_type<'c, E>(&self, ex: E) -> Result<i64>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {api_keys} ak
                INNER JOIN {user_api_keys} uak ON ak.id = uak.api_key_id",
            api_keys = API_KEY,
            user_api_keys = USER_API_KEY
        )))
        .fetch_one(ex)
        .await?;

        Ok(row.0)
    }

    pub async fn get_by_service_account_id<'c, E>(
        &self,
        ex: E,
        service_account_id: &str,
    ) -> Result<Vec<ApiKey>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, ApiKey>(sqlx::AssertSqlSafe(format!(
            "
            SELECT ak.* FROM {api_keys} ak
            INNER JOIN {sa_api_keys} sak ON ak.id = sak.api_key_id
            WHERE sak.service_account_id = $1
            ORDER BY ak.created_at DESC, ak.id DESC
        ",
            api_keys = API_KEY,
            sa_api_keys = SERVICE_ACCOUNT_API_KEY
        )))
        .bind(service_account_id)
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }

    pub async fn get_all_by_service_account_type<'c, E>(
        &self,
        ex: E,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ApiKey>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, ApiKey>(sqlx::AssertSqlSafe(format!(
            "SELECT ak.* FROM {api_keys} ak
                INNER JOIN {sa_api_keys} sak ON ak.id = sak.api_key_id
                ORDER BY ak.created_at DESC, ak.id DESC LIMIT $1 OFFSET $2",
            api_keys = API_KEY,
            sa_api_keys = SERVICE_ACCOUNT_API_KEY
        )))
        .bind(limit)
        .bind(offset)
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }

    pub async fn count_by_service_account_type<'c, E>(&self, ex: E) -> Result<i64>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {api_keys} ak
                INNER JOIN {sa_api_keys} sak ON ak.id = sak.api_key_id",
            api_keys = API_KEY,
            sa_api_keys = SERVICE_ACCOUNT_API_KEY
        )))
        .fetch_one(ex)
        .await?;

        Ok(row.0)
    }

    pub async fn get_by_hash<'c, E>(&self, ex: E, key_hash: &str) -> Result<Option<ApiKey>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, ApiKey>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {api_keys} WHERE key_hash = $1
        ",
            api_keys = API_KEY
        )))
        .bind(key_hash)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn get_auth_by_hash<'c, E>(&self, ex: E, key_hash: &str) -> Result<Option<ApiKeyAuth>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, ApiKeyAuth>(sqlx::AssertSqlSafe(format!(
            "
            SELECT ak.*, COALESCE(uak.org_id, sa.org_id) AS org_id
            FROM {api_keys} ak
            LEFT JOIN {user_api_keys} uak ON uak.api_key_id = ak.id
            LEFT JOIN {sa_api_keys} sak ON sak.api_key_id = ak.id
            LEFT JOIN {service_accounts} sa ON sa.id = sak.service_account_id
            WHERE ak.key_hash = $1
        ",
            api_keys = API_KEY,
            user_api_keys = USER_API_KEY,
            sa_api_keys = SERVICE_ACCOUNT_API_KEY,
            service_accounts = SERVICE_ACCOUNT
        )))
        .bind(key_hash)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn get_audit_by_id_for_update(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        id: &str,
    ) -> Result<Option<ApiKeyAuditRecord>> {
        let row = sqlx::query_as::<_, ApiKeyAuditRecord>(sqlx::AssertSqlSafe(format!(
            "
            SELECT ak.*,
                   CASE
                       WHEN uak.user_id IS NOT NULL THEN 'user'
                       WHEN sak.service_account_id IS NOT NULL THEN 'service_account'
                   END AS owner_type,
                   COALESCE(uak.user_id, sak.service_account_id) AS owner_id,
                   COALESCE(uak.org_id, sa.org_id) AS org_id
            FROM {api_keys} ak
            LEFT JOIN {user_api_keys} uak ON uak.api_key_id = ak.id
            LEFT JOIN {sa_api_keys} sak ON sak.api_key_id = ak.id
            LEFT JOIN {service_accounts} sa ON sa.id = sak.service_account_id
            WHERE ak.id = $1
            {for_update}
        ",
            api_keys = API_KEY,
            user_api_keys = USER_API_KEY,
            sa_api_keys = SERVICE_ACCOUNT_API_KEY,
            service_accounts = SERVICE_ACCOUNT,
            for_update = crate::backend::FOR_UPDATE_API_KEY,
        )))
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }

    /// Revoke every key owned by the user. Returns the revoked key hashes so
    /// callers can purge cached auth subjects. Run in the same transaction as
    /// the user delete: the FK cascade only removes the ownership link rows,
    /// so without this the `api_keys` rows would survive orphaned and keep
    /// authenticating.
    pub async fn revoke_by_user(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
    ) -> Result<Vec<String>> {
        let hashes: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {api_keys} SET revoked = TRUE, updated_at = $2
            WHERE revoked = FALSE
              AND id IN (SELECT api_key_id FROM {user_api_keys} WHERE user_id = $1)
            RETURNING key_hash
        ",
            api_keys = API_KEY,
            user_api_keys = USER_API_KEY
        )))
        .bind(user_id)
        .bind(Utc::now())
        .fetch_all(&mut **tx)
        .await?;

        Ok(hashes)
    }

    /// Revoke the user's keys bound to the given org (an org-bound key is
    /// revoked — not unbound — when the membership goes away, so it cannot
    /// silently degrade to an org-less credential).
    pub async fn revoke_by_user_and_org(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
        org_id: &str,
    ) -> Result<Vec<String>> {
        let hashes: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {api_keys} SET revoked = TRUE, updated_at = $3
            WHERE revoked = FALSE
              AND id IN (
                SELECT api_key_id FROM {user_api_keys}
                WHERE user_id = $1 AND org_id = $2
              )
            RETURNING key_hash
        ",
            api_keys = API_KEY,
            user_api_keys = USER_API_KEY
        )))
        .bind(user_id)
        .bind(org_id)
        .bind(Utc::now())
        .fetch_all(&mut **tx)
        .await?;

        Ok(hashes)
    }

    /// Revoke every key owned by the service account (see `revoke_by_user`
    /// for why this must run in the delete transaction).
    pub async fn revoke_by_service_account(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        service_account_id: &str,
    ) -> Result<Vec<String>> {
        let hashes: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {api_keys} SET revoked = TRUE, updated_at = $2
            WHERE revoked = FALSE
              AND id IN (
                SELECT api_key_id FROM {sa_api_keys} WHERE service_account_id = $1
              )
            RETURNING key_hash
        ",
            api_keys = API_KEY,
            sa_api_keys = SERVICE_ACCOUNT_API_KEY
        )))
        .bind(service_account_id)
        .bind(Utc::now())
        .fetch_all(&mut **tx)
        .await?;

        Ok(hashes)
    }

    /// Revoke every key acting in the org: user keys bound to it and keys of
    /// its service accounts (which the org delete is about to cascade away).
    pub async fn revoke_by_org(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        org_id: &str,
    ) -> Result<Vec<String>> {
        let mut hashes: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {api_keys} SET revoked = TRUE, updated_at = $2
            WHERE revoked = FALSE
              AND id IN (SELECT api_key_id FROM {user_api_keys} WHERE org_id = $1)
            RETURNING key_hash
        ",
            api_keys = API_KEY,
            user_api_keys = USER_API_KEY
        )))
        .bind(org_id)
        .bind(Utc::now())
        .fetch_all(&mut **tx)
        .await?;

        let sa_hashes: Vec<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {api_keys} SET revoked = TRUE, updated_at = $2
            WHERE revoked = FALSE
              AND id IN (
                SELECT sak.api_key_id FROM {sa_api_keys} sak
                INNER JOIN {service_accounts} sa ON sa.id = sak.service_account_id
                WHERE sa.org_id = $1
              )
            RETURNING key_hash
        ",
            api_keys = API_KEY,
            sa_api_keys = SERVICE_ACCOUNT_API_KEY,
            service_accounts = SERVICE_ACCOUNT
        )))
        .bind(org_id)
        .bind(Utc::now())
        .fetch_all(&mut **tx)
        .await?;

        hashes.extend(sa_hashes);
        Ok(hashes)
    }

    pub async fn revoke_by_id(&self, tx: &mut crate::backend::Tx<'_>, id: &str) -> Result<()> {
        let updated_at = Utc::now();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {api_keys} SET revoked = TRUE, updated_at = $2 WHERE id = $1
        ",
            api_keys = API_KEY
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
            DELETE FROM {api_keys} WHERE id = $1
        ",
            api_keys = API_KEY
        )))
        .bind(id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn get_by_id<'c, E>(&self, ex: E, id: &str) -> Result<Option<ApiKey>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, ApiKey>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {api_keys} WHERE id = $1
        ",
            api_keys = API_KEY
        )))
        .bind(id)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn search<'c, E>(
        &self,
        ex: E,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ApiKey>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let pattern = crate::backend::like_contains(query);
        let rows = sqlx::query_as::<_, ApiKey>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {api_keys}
            WHERE label {like} $1 {esc}
            ORDER BY created_at DESC, id DESC LIMIT $2 OFFSET $3
        ",
            api_keys = API_KEY,
            like = crate::backend::LIKE,
            esc = crate::backend::LIKE_ESCAPE
        )))
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }

    pub async fn count_search<'c, E>(&self, ex: E, query: &str) -> Result<i64>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let pattern = crate::backend::like_contains(query);
        let row: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "
            SELECT COUNT(*) FROM {api_keys}
            WHERE label {like} $1 {esc}
        ",
            api_keys = API_KEY,
            like = crate::backend::LIKE,
            esc = crate::backend::LIKE_ESCAPE
        )))
        .bind(&pattern)
        .fetch_one(ex)
        .await?;

        Ok(row.0)
    }

    pub async fn get_all<'c, E>(&self, ex: E, limit: i64, offset: i64) -> Result<Vec<ApiKey>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, ApiKey>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {api_keys} ORDER BY created_at DESC, id DESC LIMIT $1 OFFSET $2
        ",
            api_keys = API_KEY
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
            SELECT COUNT(*) FROM {api_keys}
        ",
            api_keys = API_KEY
        )))
        .fetch_one(ex)
        .await?;

        Ok(row.0)
    }

    pub async fn update(&self, tx: &mut crate::backend::Tx<'_>, api_key: ApiKey) -> Result<ApiKey> {
        let updated_at = Utc::now();
        let row = sqlx::query_as::<_, ApiKey>(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {api_keys}
            SET label = $2, attrs = $3, updated_at = $4
            WHERE id = $1
            RETURNING *
        ",
            api_keys = API_KEY
        )))
        .bind(&api_key.id)
        .bind(&api_key.label)
        .bind(&api_key.attrs)
        .bind(updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }
}
