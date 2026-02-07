use crate::ent::{Account, AccountType};
use anyhow::Result;

pub const ACCOUNT: &str = "accounts";

#[derive(Clone)]
pub struct AccountRepository {}

impl AccountRepository {
    pub fn new() -> Self {
        Self {}
    }

    pub async fn get_by_id(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
    ) -> Result<Option<Account>> {
        let row = sqlx::query_as::<_, Account>(
            format!(
                "
            SELECT * FROM {accounts} WHERE id = $1
        ",
                accounts = ACCOUNT
            )
            .as_str(),
        )
        .bind(id)
        .fetch_optional(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn create(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account_type: AccountType,
        name: &str,
        description: Option<&str>,
    ) -> Result<Account> {
        let account = Account::new(
            name.to_string(),
            account_type,
            description.map(|d| d.to_string()),
        );

        let row = sqlx::query_as::<_, Account>(
            format!(
                "
            INSERT INTO {accounts} (id, type, name, description)
            VALUES ($1, $2, $3, $4)
            RETURNING *
        ",
                accounts = ACCOUNT
            )
            .as_str(),
        )
        .bind(&account.id)
        .bind(&account.account_type)
        .bind(&account.name)
        .bind(&account.description)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn update(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
        account_type: AccountType,
        name: &str,
        description: Option<&str>,
    ) -> Result<Account> {
        let account = Account::new(
            name.to_string(),
            account_type,
            description.map(|d| d.to_string()),
        );

        let row = sqlx::query_as::<_, Account>(
            format!(
                "
            UPDATE {accounts} SET type = $2, name = $3, description = $4 WHERE id = $1
            RETURNING *
        ",
                accounts = ACCOUNT
            )
            .as_str(),
        )
        .bind(id)
        .bind(&account.account_type)
        .bind(&account.name)
        .bind(&account.description)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row)
    }

    pub async fn get_all_by_type(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account_type: AccountType,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Account>> {
        let rows = sqlx::query_as::<_, Account>(
            format!(
                "
            SELECT * FROM {accounts} WHERE type = $1 ORDER BY id DESC LIMIT $2 OFFSET $3
        ",
                accounts = ACCOUNT
            )
            .as_str(),
        )
        .bind(account_type)
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut **tx)
        .await?;

        Ok(rows)
    }

    pub async fn count_by_type(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account_type: AccountType,
    ) -> Result<i64> {
        let row: (i64,) = sqlx::query_as(
            format!(
                "
            SELECT COUNT(*) FROM {accounts} WHERE type = $1
        ",
                accounts = ACCOUNT
            )
            .as_str(),
        )
        .bind(account_type)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row.0)
    }

    pub async fn search_by_type(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account_type: AccountType,
        query: &str,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<Account>> {
        let pattern = format!("%{}%", query);
        let rows = sqlx::query_as::<_, Account>(
            format!(
                "
            SELECT * FROM {accounts}
            WHERE type = $1 AND (name ILIKE $2 OR description ILIKE $2)
            ORDER BY id DESC LIMIT $3 OFFSET $4
        ",
                accounts = ACCOUNT
            )
            .as_str(),
        )
        .bind(account_type)
        .bind(&pattern)
        .bind(limit)
        .bind(offset)
        .fetch_all(&mut **tx)
        .await?;

        Ok(rows)
    }

    pub async fn count_search_by_type(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        account_type: AccountType,
        query: &str,
    ) -> Result<i64> {
        let pattern = format!("%{}%", query);
        let row: (i64,) = sqlx::query_as(
            format!(
                "
            SELECT COUNT(*) FROM {accounts}
            WHERE type = $1 AND (name ILIKE $2 OR description ILIKE $2)
        ",
                accounts = ACCOUNT
            )
            .as_str(),
        )
        .bind(account_type)
        .bind(&pattern)
        .fetch_one(&mut **tx)
        .await?;

        Ok(row.0)
    }

    pub async fn delete(
        &self,
        #[cfg(feature = "sqlite")] tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
        #[cfg(feature = "postgres")] tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        id: &str,
    ) -> Result<()> {
        sqlx::query(format!("DELETE FROM {accounts} WHERE id = $1", accounts = ACCOUNT).as_str())
            .bind(id)
            .execute(&mut **tx)
            .await?;

        Ok(())
    }
}
