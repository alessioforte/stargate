use crate::ent::OAuthClient;
use anyhow::Result;
use chrono::{DateTime, Utc};

pub const OAUTH_CLIENT: &str = "oauth_clients";

#[derive(Clone)]
pub struct OAuthClientRepository {}

impl OAuthClientRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn create(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        client: OAuthClient,
    ) -> Result<OAuthClient> {
        let row = sqlx::query_as::<_, OAuthClient>(sqlx::AssertSqlSafe(format!(
            "
            INSERT INTO {tbl} (
                client_id,
                client_secret_hash,
                name,
                description,
                enabled,
                token_endpoint_auth_method,
                grant_types,
                response_types,
                redirect_uris,
                scopes,
                audiences,
                attrs,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            RETURNING *
        ",
            tbl = OAUTH_CLIENT
        )))
        .bind(&client.client_id)
        .bind(&client.client_secret_hash)
        .bind(&client.name)
        .bind(&client.description)
        .bind(client.enabled)
        .bind(&client.token_endpoint_auth_method)
        .bind(&client.grant_types)
        .bind(&client.response_types)
        .bind(&client.redirect_uris)
        .bind(&client.scopes)
        .bind(&client.audiences)
        .bind(&client.attrs)
        .bind(client.created_at)
        .bind(client.updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_client_id<'c, E>(
        &self,
        ex: E,
        client_id: &str,
    ) -> Result<Option<OAuthClient>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, OAuthClient>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {tbl} WHERE client_id = $1",
            tbl = OAUTH_CLIENT
        )))
        .bind(client_id)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn get_all<'c, E>(&self, ex: E, limit: i64, offset: i64) -> Result<Vec<OAuthClient>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, OAuthClient>(sqlx::AssertSqlSafe(format!(
            "SELECT * FROM {tbl} ORDER BY created_at DESC, client_id DESC LIMIT $1 OFFSET $2",
            tbl = OAUTH_CLIENT
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
            tbl = OAUTH_CLIENT
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
    ) -> Result<Vec<OAuthClient>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let pattern = crate::backend::like_contains(query);
        let rows = sqlx::query_as::<_, OAuthClient>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {tbl}
            WHERE client_id {like} $1 {esc} OR name {like} $1 {esc} OR description {like} $1 {esc}
            ORDER BY created_at DESC, client_id DESC LIMIT $2 OFFSET $3
        ",
            tbl = OAUTH_CLIENT,
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
            SELECT COUNT(*) FROM {tbl}
            WHERE client_id {like} $1 {esc} OR name {like} $1 {esc} OR description {like} $1 {esc}
        ",
            tbl = OAUTH_CLIENT,
            like = crate::backend::LIKE,
            esc = crate::backend::LIKE_ESCAPE
        )))
        .bind(&pattern)
        .fetch_one(ex)
        .await?;

        Ok(row.0)
    }

    pub async fn update(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        client: OAuthClient,
    ) -> Result<OAuthClient> {
        let updated_at = Utc::now();
        let row = sqlx::query_as::<_, OAuthClient>(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {tbl}
            SET name = $2,
                description = $3,
                token_endpoint_auth_method = $4,
                grant_types = $5,
                response_types = $6,
                redirect_uris = $7,
                scopes = $8,
                audiences = $9,
                attrs = $10,
                client_secret_hash = $11,
                updated_at = $12
            WHERE client_id = $1
            RETURNING *
        ",
            tbl = OAUTH_CLIENT
        )))
        .bind(&client.client_id)
        .bind(&client.name)
        .bind(&client.description)
        .bind(&client.token_endpoint_auth_method)
        .bind(&client.grant_types)
        .bind(&client.response_types)
        .bind(&client.redirect_uris)
        .bind(&client.scopes)
        .bind(&client.audiences)
        .bind(&client.attrs)
        .bind(&client.client_secret_hash)
        .bind(updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn update_secret_hash(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        client_id: &str,
        client_secret_hash: Option<&str>,
    ) -> Result<OAuthClient> {
        let updated_at: DateTime<Utc> = Utc::now();
        let row = sqlx::query_as::<_, OAuthClient>(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {tbl}
            SET client_secret_hash = $2, updated_at = $3
            WHERE client_id = $1
            RETURNING *
        ",
            tbl = OAUTH_CLIENT
        )))
        .bind(client_id)
        .bind(client_secret_hash)
        .bind(updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn set_enabled(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        client_id: &str,
        enabled: bool,
    ) -> Result<OAuthClient> {
        let updated_at = Utc::now();
        let row = sqlx::query_as::<_, OAuthClient>(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {tbl}
            SET enabled = $2, updated_at = $3
            WHERE client_id = $1
            RETURNING *
        ",
            tbl = OAUTH_CLIENT
        )))
        .bind(client_id)
        .bind(enabled)
        .bind(updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn delete_by_client_id(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        client_id: &str,
    ) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {tbl} WHERE client_id = $1",
            tbl = OAUTH_CLIENT
        )))
        .bind(client_id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}

impl Default for OAuthClientRepository {
    fn default() -> Self {
        Self::new()
    }
}
