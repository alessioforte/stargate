use crate::ent::ServiceAccount;
use anyhow::Result;

pub const SERVICE_ACCOUNT: &str = "service_accounts";

#[derive(Clone)]
pub struct ServiceAccountRepository {}

impl ServiceAccountRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for ServiceAccountRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl ServiceAccountRepository {
    pub async fn create(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        name: &str,
        description: Option<&str>,
        org_id: Option<&str>,
    ) -> Result<ServiceAccount> {
        let sa = ServiceAccount::new(
            name.to_string(),
            description.map(|d| d.to_string()),
            org_id.map(|o| o.to_string()),
        );

        let row = sqlx::query_as::<_, ServiceAccount>(sqlx::AssertSqlSafe(format!(
            "
            INSERT INTO {tbl} (id, name, description, org_id)
            VALUES ($1, $2, $3, $4)
            RETURNING *
        ",
            tbl = SERVICE_ACCOUNT
        )))
        .bind(&sa.id)
        .bind(&sa.name)
        .bind(&sa.description)
        .bind(&sa.org_id)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_id<'c, E>(&self, ex: E, id: &str) -> Result<Option<ServiceAccount>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, ServiceAccount>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {tbl} WHERE id = $1",
            tbl = SERVICE_ACCOUNT
        )))
        .bind(id)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn get_all<'c, E>(
        &self,
        ex: E,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ServiceAccount>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, ServiceAccount>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {tbl} ORDER BY id DESC LIMIT $1 OFFSET $2",
            tbl = SERVICE_ACCOUNT
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
            "SELECT COUNT(*) FROM {tbl}",
            tbl = SERVICE_ACCOUNT
        )))
        .fetch_one(ex)
        .await?;

        Ok(row.0)
    }

    pub async fn search<'c, E>(
        &self,
        ex: E,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<ServiceAccount>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query_as::<_, ServiceAccount>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {tbl}
            WHERE name {like} $1 OR description {like} $1
            ORDER BY id DESC LIMIT $2 OFFSET $3
        ",
            tbl = SERVICE_ACCOUNT,
            like = crate::backend::LIKE
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
        let pattern = format!("%{}%", query);
        let row: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "
            SELECT COUNT(*) FROM {tbl}
            WHERE name {like} $1 OR description {like} $1
        ",
            tbl = SERVICE_ACCOUNT,
            like = crate::backend::LIKE
        )))
        .bind(&pattern)
        .fetch_one(ex)
        .await?;

        Ok(row.0)
    }

    pub async fn update(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        id: &str,
        name: &str,
        description: Option<&str>,
        org_id: Option<&str>,
    ) -> Result<ServiceAccount> {
        let row = sqlx::query_as::<_, ServiceAccount>(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {tbl} SET name = $2, description = $3, org_id = $4 WHERE id = $1
            RETURNING *
        ",
            tbl = SERVICE_ACCOUNT
        )))
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(org_id)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn delete(&self, tx: &mut crate::backend::Tx<'_>, id: &str) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {tbl} WHERE id = $1",
            tbl = SERVICE_ACCOUNT
        )))
        .bind(id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}
