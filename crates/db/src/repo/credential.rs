use crate::ent::{Credential, CredentialType};
use anyhow::Result;

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

        let row = sqlx::query_as::<_, Credential>(
            format!(
                "
            INSERT INTO {credentials} (id, user_id, type, value)
            VALUES ($1, $2, $3, $4)
            RETURNING *
        ",
                credentials = CREDENTIAL
            )
            .as_str(),
        )
        .bind(&credential.id)
        .bind(&credential.user_id)
        .bind(&credential.credential_type)
        .bind(&credential.value)
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
        let row = sqlx::query_as::<_, Credential>(
            format!(
                "
            SELECT * FROM {credentials} WHERE user_id = $1 AND type = $2
        ",
                credentials = CREDENTIAL
            )
            .as_str(),
        )
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
        let row = sqlx::query_as::<_, Credential>(
            format!(
                "
            UPDATE {credentials}
            SET value = $1
            WHERE user_id = $2 AND type = $3
            RETURNING *
        ",
                credentials = CREDENTIAL
            )
            .as_str(),
        )
        .bind(new_password)
        .bind(user_id)
        .bind(CredentialType::Password)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn delete_by_user_id(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
    ) -> Result<()> {
        sqlx::query(
            format!(
                "DELETE FROM {credentials} WHERE user_id = $1",
                credentials = CREDENTIAL
            )
            .as_str(),
        )
        .bind(user_id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}
