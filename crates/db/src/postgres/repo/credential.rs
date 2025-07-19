use crate::ent::{Credential, CredentialType};
use anyhow::Result;

#[derive(Clone)]
pub struct CredentialRepository {}

impl CredentialRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn create(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: &str,
        credential_type: CredentialType,
        value: &str,
    ) -> Result<Credential> {
        let credential = Credential::new(user_id.to_string(), credential_type, value.to_string());

        let row = sqlx::query_as::<_, Credential>(
            "
            INSERT INTO credentials (id, user_id, timestamp, type, value)
            VALUES ($1, $2, $3, $4, $5)
            RETURNING *
        ",
        )
        .bind(&credential.id)
        .bind(&credential.user_id)
        .bind(credential.timestamp)
        .bind(&credential.credential_type)
        .bind(&credential.value)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_user_id(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: &str,
        credential_type: CredentialType,
    ) -> Result<Option<Credential>> {
        let ct = credential_type.as_str();
        let row = sqlx::query_as::<_, Credential>(
            "
            SELECT * FROM credentials WHERE user_id = $1 AND type = $2
        ",
        )
        .bind(user_id)
        .bind(ct)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn change_password(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        user_id: &str,
        new_password: &str,
    ) -> Result<Credential> {
        let credential = Credential::new(
            user_id.to_string(),
            CredentialType::Password, // Assuming Password type for simplicity
            new_password.to_string(),
        );

        let row = sqlx::query_as::<_, Credential>(
            "
            UPDATE credentials
            SET value = $1, timestamp = $2
            WHERE user_id = $3 AND type = $4
            RETURNING *
        ",
        )
        .bind(&credential.value)
        .bind(credential.timestamp)
        .bind(&credential.user_id)
        .bind(&credential.credential_type)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }
}
