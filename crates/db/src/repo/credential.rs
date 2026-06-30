use crate::ent::{Credential, CredentialType};
use anyhow::Result;
use chrono::Utc;

pub const CREDENTIAL: &str = "credentials";

#[derive(Clone)]
pub struct CredentialRepository {}

impl CredentialRepository {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for CredentialRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl CredentialRepository {
    pub async fn create(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
        credential_type: CredentialType,
        value: &str,
    ) -> Result<Credential> {
        let credential = Credential::new(user_id.to_string(), credential_type, value.to_string());

        let row = sqlx::query_as::<_, Credential>(sqlx::AssertSqlSafe(format!(
            "
            INSERT INTO {credentials} (id, user_id, type, value, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
        ",
            credentials = CREDENTIAL
        )))
        .bind(&credential.id)
        .bind(&credential.user_id)
        .bind(&credential.credential_type)
        .bind(&credential.value)
        .bind(credential.created_at)
        .bind(credential.updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_user_id<'c, E>(
        &self,
        ex: E,
        user_id: &str,
        credential_type: CredentialType,
    ) -> Result<Option<Credential>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let row = sqlx::query_as::<_, Credential>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {credentials} WHERE user_id = $1 AND type = $2
        ",
            credentials = CREDENTIAL
        )))
        .bind(user_id)
        .bind(credential_type)
        .fetch_optional(ex)
        .await?;

        Ok(row)
    }

    pub async fn change_password(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
        new_password: &str,
    ) -> Result<Credential> {
        let updated_at = Utc::now();
        let row = sqlx::query_as::<_, Credential>(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {credentials}
            SET value = $1, updated_at = $4
            WHERE user_id = $2 AND type = $3
            RETURNING *
        ",
            credentials = CREDENTIAL
        )))
        .bind(new_password)
        .bind(user_id)
        .bind(CredentialType::Password)
        .bind(updated_at)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn delete_by_user_id(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
    ) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DELETE FROM {credentials} WHERE user_id = $1",
            credentials = CREDENTIAL
        )))
        .bind(user_id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}
