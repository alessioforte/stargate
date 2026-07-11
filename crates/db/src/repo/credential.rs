use crate::ent::{Credential, CredentialHistory, CredentialType};
use anyhow::Result;
use chrono::Utc;

pub const CREDENTIAL: &str = "credentials";
pub const CREDENTIAL_HISTORY: &str = "credential_history";

/// Maximum previous password hashes kept per user; older rows are pruned on
/// password change. Bounds how deep a `history` password policy can look.
pub const CREDENTIAL_HISTORY_KEEP: i64 = 24;

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

    /// Replace the stored password hash without touching `updated_at` (which
    /// doubles as the password-changed timestamp for expiry) or history: this
    /// is a hash upgrade, not a password change. Guarded on the old value so
    /// concurrent rehashes can't clobber a newer hash; returns whether a row
    /// was updated.
    pub async fn rehash_value(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
        old_value: &str,
        new_value: &str,
    ) -> Result<bool> {
        let result = sqlx::query(sqlx::AssertSqlSafe(format!(
            "
            UPDATE {credentials}
            SET value = $1
            WHERE user_id = $2 AND type = $3 AND value = $4
        ",
            credentials = CREDENTIAL
        )))
        .bind(new_value)
        .bind(user_id)
        .bind(CredentialType::Password)
        .bind(old_value)
        .execute(&mut **tx)
        .await?;

        Ok(result.rows_affected() > 0)
    }

    pub async fn insert_history(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
        value: &str,
    ) -> Result<()> {
        let entry = CredentialHistory::new(user_id.to_string(), value.to_string());

        sqlx::query(sqlx::AssertSqlSafe(format!(
            "
            INSERT INTO {history} (id, user_id, value, created_at)
            VALUES ($1, $2, $3, $4)
        ",
            history = CREDENTIAL_HISTORY
        )))
        .bind(&entry.id)
        .bind(&entry.user_id)
        .bind(&entry.value)
        .bind(entry.created_at)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }

    pub async fn get_history<'c, E>(
        &self,
        ex: E,
        user_id: &str,
        limit: i64,
    ) -> Result<Vec<CredentialHistory>>
    where
        E: crate::backend::ReadExecutor<'c>,
    {
        let rows = sqlx::query_as::<_, CredentialHistory>(sqlx::AssertSqlSafe(format!(
            "
            SELECT * FROM {history}
            WHERE user_id = $1
            ORDER BY created_at DESC, id DESC
            LIMIT $2
        ",
            history = CREDENTIAL_HISTORY
        )))
        .bind(user_id)
        .bind(limit)
        .fetch_all(ex)
        .await?;

        Ok(rows)
    }

    pub async fn prune_history(
        &self,
        tx: &mut crate::backend::Tx<'_>,
        user_id: &str,
        keep: i64,
    ) -> Result<()> {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "
            DELETE FROM {history}
            WHERE user_id = $1 AND id NOT IN (
                SELECT id FROM {history}
                WHERE user_id = $1
                ORDER BY created_at DESC, id DESC
                LIMIT $2
            )
        ",
            history = CREDENTIAL_HISTORY
        )))
        .bind(user_id)
        .bind(keep)
        .execute(&mut **tx)
        .await?;

        Ok(())
    }
}
