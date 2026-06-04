use crate::ent::OAuthConsent;
use anyhow::Result;
use chrono::{DateTime, Utc};

pub const OAUTH_CONSENT: &str = "oauth_consents";

#[derive(Clone)]
pub struct OAuthConsentRepository {}

impl OAuthConsentRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn upsert(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        consent: OAuthConsent,
    ) -> Result<OAuthConsent> {
        let row = sqlx::query_as::<_, OAuthConsent>(sqlx::AssertSqlSafe(format!(
            "
            INSERT INTO {tbl} (
                id,
                client_id,
                user_id,
                scopes,
                audiences,
                granted_at,
                expires_at,
                revoked_at,
                attrs,
                created_at,
                updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)
            ON CONFLICT (client_id, user_id) DO UPDATE
            SET scopes = excluded.scopes,
                audiences = excluded.audiences,
                granted_at = excluded.granted_at,
                expires_at = excluded.expires_at,
                revoked_at = NULL,
                attrs = excluded.attrs,
                updated_at = excluded.updated_at
            RETURNING *
        ",
            tbl = OAUTH_CONSENT
        )))
        .bind(&consent.id)
        .bind(&consent.client_id)
        .bind(&consent.user_id)
        .bind(&consent.scopes)
        .bind(&consent.audiences)
        .bind(consent.granted_at)
        .bind(consent.expires_at)
        .bind(consent.revoked_at)
        .bind(&consent.attrs)
        .bind(consent.created_at)
        .bind(consent.updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_active_for_user_client<'c, E>(
        &self,
        ex: E,
        user_id: &str,
        client_id: &str,
        now: DateTime<Utc>,
    ) -> Result<Option<OAuthConsent>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, OAuthConsent>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {tbl}
            WHERE user_id = $1
              AND client_id = $2
              AND revoked_at IS NULL
              AND (expires_at IS NULL OR expires_at > $3)
        ",
            tbl = OAUTH_CONSENT
        )))
        .bind(user_id)
        .bind(client_id)
        .bind(now)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn revoke_for_user_client(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
        client_id: &str,
    ) -> Result<()> {
        let now = Utc::now();
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {tbl}
            SET revoked_at = $3, updated_at = $3
            WHERE user_id = $1 AND client_id = $2 AND revoked_at IS NULL
        ",
            tbl = OAUTH_CONSENT
        )))
        .bind(user_id)
        .bind(client_id)
        .bind(now)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}

impl Default for OAuthConsentRepository {
    fn default() -> Self {
        Self::new()
    }
}
