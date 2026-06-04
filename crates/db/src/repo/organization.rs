use super::USER;
use crate::ent::{Organization, User};
use anyhow::Result;
pub const ORGANIZATION: &str = "organizations";
pub const USER_ORGANIZATION: &str = "user_organizations";

#[derive(Clone)]
pub struct OrganizationRepository {}

impl OrganizationRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for OrganizationRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl OrganizationRepository {
    pub async fn create(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        name: &str,
        description: Option<&str>,
        attrs: Option<&serde_json::Value>,
    ) -> Result<Organization> {
        let org = Organization::new(name.to_string(), description.map(|d| d.to_string()));

        let row = sqlx::query_as::<_, Organization>(sqlx::AssertSqlSafe(format!(
            "
            INSERT INTO {tbl} (id, name, description, attrs)
            VALUES ($1, $2, $3, $4)
            RETURNING *
        ",
            tbl = ORGANIZATION
        )))
        .bind(&org.id)
        .bind(&org.name)
        .bind(&org.description)
        .bind(attrs)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_id<'c, E>(&self, ex: E, id: &str) -> Result<Option<Organization>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, Organization>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {tbl} WHERE id = $1",
            tbl = ORGANIZATION
        )))
        .bind(id)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn get_all<'c, E>(&self, ex: E, limit: i64, offset: i64) -> Result<Vec<Organization>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, Organization>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {tbl} ORDER BY id DESC LIMIT $1 OFFSET $2",
            tbl = ORGANIZATION
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
            tbl = ORGANIZATION
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
    ) -> Result<Vec<Organization>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query_as::<_, Organization>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {tbl}
            WHERE name {like} $1 OR description {like} $1
            ORDER BY id DESC LIMIT $2 OFFSET $3
        ",
            tbl = ORGANIZATION,
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
            tbl = ORGANIZATION,
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
        attrs: Option<&serde_json::Value>,
    ) -> Result<Organization> {
        let row = sqlx::query_as::<_, Organization>(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {tbl} SET name = $2, description = $3, attrs = $4 WHERE id = $1
            RETURNING *
        ",
            tbl = ORGANIZATION
        )))
        .bind(id)
        .bind(name)
        .bind(description)
        .bind(attrs)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn delete(&self, tx: &mut crate::backend::Tx<'_>, id: &str) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {tbl} WHERE id = $1",
            tbl = ORGANIZATION
        )))
        .bind(id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn add_user(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
        org_id: &str,
    ) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "INSERT INTO {tbl} (user_id, org_id) VALUES ($1, $2)",
            tbl = USER_ORGANIZATION
        )))
        .bind(user_id)
        .bind(org_id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn remove_user(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
        org_id: &str,
    ) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {tbl} WHERE user_id = $1 AND org_id = $2",
            tbl = USER_ORGANIZATION
        )))
        .bind(user_id)
        .bind(org_id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn get_users<'c, E>(&self, ex: E, org_id: &str) -> Result<Vec<User>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, User>(sqlx::AssertSqlSafe(format!(
            "
            SELECT u.* FROM {users} u
            INNER JOIN {tbl} uo ON u.id = uo.user_id
            WHERE uo.org_id = $1
            ORDER BY u.id
        ",
            users = USER,
            tbl = USER_ORGANIZATION
        )))
        .bind(org_id)
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }

    pub async fn get_users_paginated<'c, E>(
        &self,
        ex: E,
        org_id: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<User>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, User>(sqlx::AssertSqlSafe(format!(
            "SELECT u.* FROM {users} u
                INNER JOIN {tbl} uo ON u.id = uo.user_id
                WHERE uo.org_id = $1
                ORDER BY u.id DESC LIMIT $2 OFFSET $3",
            users = USER,
            tbl = USER_ORGANIZATION
        )))
        .bind(org_id)
        .bind(limit)
        .bind(offset)
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }

    pub async fn count_users<'c, E>(&self, ex: E, org_id: &str) -> Result<i64>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row: (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {users} u
                INNER JOIN {tbl} uo ON u.id = uo.user_id
                WHERE uo.org_id = $1",
            users = USER,
            tbl = USER_ORGANIZATION
        )))
        .bind(org_id)
        .fetch_one(ex)
        .await?;

        Ok(row.0)
    }

    pub async fn get_orgs_by_user<'c, E>(&self, ex: E, user_id: &str) -> Result<Vec<Organization>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, Organization>(sqlx::AssertSqlSafe(format!(
            "
            SELECT o.* FROM {tbl_org} o
            INNER JOIN {tbl_uo} uo ON o.id = uo.org_id
            WHERE uo.user_id = $1
            ORDER BY o.id
        ",
            tbl_org = ORGANIZATION,
            tbl_uo = USER_ORGANIZATION
        )))
        .bind(user_id)
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }
}
