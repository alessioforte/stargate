use crate::ent::ApiKey;
use anyhow::Result;

#[derive(Clone)]
pub struct ApiKeyRepository {}

impl ApiKeyRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn create(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account_id: &str,
        key_hash: &str,
        label: Option<String>,
        attrs: Option<serde_json::Value>,
    ) -> Result<ApiKey> {
        let api_key = ApiKey::new(
            key_hash.to_string(),
            account_id.to_string(),
            label,
            attrs.unwrap_or(serde_json::Value::Null),
        );
        let row = sqlx::query_as::<_, ApiKey>(
            "
            INSERT INTO api_keys (id, account_id, key_hash, label, revoked, attrs)
            VALUES ($1, $2, $3, $4, $5, $6)
            RETURNING *
        ",
        )
        .bind(&api_key.id)
        .bind(&api_key.account_id)
        .bind(&api_key.key_hash)
        .bind(&api_key.label)
        .bind(&api_key.revoked)
        .bind(&api_key.attrs)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_by_hash(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        key_hash: &str,
    ) -> Result<Option<ApiKey>> {
        let row = sqlx::query_as::<_, ApiKey>(
            "
            SELECT * FROM api_keys WHERE key_hash = $1
        ",
        )
        .bind(key_hash)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn revoke_by_id(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
    ) -> Result<()> {
        sqlx::query(
            "
            UPDATE api_keys SET revoked = TRUE WHERE id = $1
        ",
        )
        .bind(id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn delete_by_id(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
    ) -> Result<()> {
        sqlx::query(
            "
            DELETE FROM api_keys WHERE id = $1
        ",
        )
        .bind(id)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}
