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
        let row = sqlx::query_as::<_, OAuthClient>(
            format!(
                "
            INSERT INTO {tbl} (
                client_id,
                client_secret_hash,
                name,
                description,
                org_id,
                service_account_id,
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
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16)
            RETURNING *
        ",
                tbl = OAUTH_CLIENT
            )
            .as_str(),
        )
        .bind(&client.client_id)
        .bind(&client.client_secret_hash)
        .bind(&client.name)
        .bind(&client.description)
        .bind(&client.org_id)
        .bind(&client.service_account_id)
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
        let row = sqlx::query_as::<_, OAuthClient>(
            format!(
                "SELECT * FROM {tbl} WHERE client_id = $1",
                tbl = OAUTH_CLIENT
            )
            .as_str(),
        )
        .bind(client_id)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn get_all<'c, E>(&self, ex: E, limit: i64, offset: i64) -> Result<Vec<OAuthClient>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, OAuthClient>(
            format!(
                "SELECT * FROM {tbl} ORDER BY created_at DESC, client_id DESC LIMIT $1 OFFSET $2",
                tbl = OAUTH_CLIENT
            )
            .as_str(),
        )
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
        let row: (i64,) =
            sqlx::query_as(format!("SELECT COUNT(*) FROM {tbl}", tbl = OAUTH_CLIENT).as_str())
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
        let pattern = format!("%{}%", query);
        let rows = sqlx::query_as::<_, OAuthClient>(
            format!(
                "
            SELECT * FROM {tbl}
            WHERE client_id {like} $1 OR name {like} $1 OR description {like} $1
            ORDER BY created_at DESC, client_id DESC LIMIT $2 OFFSET $3
        ",
                tbl = OAUTH_CLIENT,
                like = crate::backend::LIKE
            )
            .as_str(),
        )
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
        let row: (i64,) = sqlx::query_as(
            format!(
                "
            SELECT COUNT(*) FROM {tbl}
            WHERE client_id {like} $1 OR name {like} $1 OR description {like} $1
        ",
                tbl = OAUTH_CLIENT,
                like = crate::backend::LIKE
            )
            .as_str(),
        )
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
        let row = sqlx::query_as::<_, OAuthClient>(
            format!(
                "
            UPDATE {tbl}
            SET name = $2,
                description = $3,
                org_id = $4,
                service_account_id = $5,
                token_endpoint_auth_method = $6,
                grant_types = $7,
                response_types = $8,
                redirect_uris = $9,
                scopes = $10,
                audiences = $11,
                attrs = $12,
                client_secret_hash = $13,
                updated_at = $14
            WHERE client_id = $1
            RETURNING *
        ",
                tbl = OAUTH_CLIENT
            )
            .as_str(),
        )
        .bind(&client.client_id)
        .bind(&client.name)
        .bind(&client.description)
        .bind(&client.org_id)
        .bind(&client.service_account_id)
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
        let row = sqlx::query_as::<_, OAuthClient>(
            format!(
                "
            UPDATE {tbl}
            SET client_secret_hash = $2, updated_at = $3
            WHERE client_id = $1
            RETURNING *
        ",
                tbl = OAUTH_CLIENT
            )
            .as_str(),
        )
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
        let row = sqlx::query_as::<_, OAuthClient>(
            format!(
                "
            UPDATE {tbl}
            SET enabled = $2, updated_at = $3
            WHERE client_id = $1
            RETURNING *
        ",
                tbl = OAUTH_CLIENT
            )
            .as_str(),
        )
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
        sqlx::query(format!("DELETE FROM {tbl} WHERE client_id = $1", tbl = OAUTH_CLIENT).as_str())
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
